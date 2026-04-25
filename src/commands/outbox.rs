use anyhow::{Context, Result, bail};
use rusqlite::params;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app::App;
use crate::cli::OutboxRetryArgs;
use crate::models::SendEmailRequest;
use crate::output::print_success_or;

impl App {
    /// Write a send intent to the outbox with a stable idempotency key,
    /// then return the key for immediate delivery attempt.
    pub fn outbox_send(&self, request: &SendEmailRequest, account_email: &str) -> Result<String> {
        // Opportunistically upgrade the idempotency-key index to UNIQUE. It's
        // a no-op once done, and we want the DB-level guard in place before
        // the first insert (ritalin O-010).
        let _ = crate::db::ensure_outbox_unique_index(&self.conn);

        let request_json = serde_json::to_string(request)?;
        let idempotency_key = stable_idempotency_key(request);
        let id = Uuid::new_v4().to_string();

        // Application-level dedup: if a row with this idempotency_key already
        // exists, return its key without inserting a new one. Closes the gap
        // where `ensure_outbox_unique_index` falls back to a non-unique index
        // on installs that had pre-existing duplicate rows — in that case
        // the DB wouldn't reject duplicates, so we enforce here. Also guards
        // against the race where two threads try to outbox_send the same
        // request concurrently (identical key, identical hash).
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM outbox WHERE idempotency_key = ?1 LIMIT 1",
                params![idempotency_key],
                |row| row.get(0),
            )
            .ok();
        if existing.is_some() {
            return Ok(idempotency_key);
        }

        self.conn.execute(
            "INSERT INTO outbox (id, account_email, request_json, idempotency_key, status)
             VALUES (?1, ?2, ?3, ?4, 'pending')",
            params![id, account_email, request_json, idempotency_key],
        )?;

        Ok(idempotency_key)
    }

    /// Mark an outbox entry as sent
    pub fn outbox_mark_sent(&self, idempotency_key: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE outbox SET status = 'sent', updated_at = CURRENT_TIMESTAMP WHERE idempotency_key = ?1",
            params![idempotency_key],
        )?;
        Ok(())
    }

    /// Mark an outbox entry as failed
    pub fn outbox_mark_failed(&self, idempotency_key: &str, error: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE outbox SET status = 'failed', last_error = ?1, attempts = attempts + 1, updated_at = CURRENT_TIMESTAMP WHERE idempotency_key = ?2",
            params![error, idempotency_key],
        )?;
        Ok(())
    }

    pub fn outbox_list(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_email, idempotency_key, status, attempts, last_error, created_at
             FROM outbox ORDER BY created_at DESC",
        )?;

        #[derive(serde::Serialize)]
        struct OutboxEntry {
            id: String,
            account_email: String,
            status: String,
            attempts: i64,
            last_error: Option<String>,
            created_at: String,
        }

        let rows = stmt.query_map([], |row| {
            Ok(OutboxEntry {
                id: row.get(0)?,
                account_email: row.get(1)?,
                status: row.get(3)?,
                attempts: row.get(4)?,
                last_error: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        let entries: Vec<_> = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        print_success_or(self.format, &entries, |entries| {
            for entry in entries {
                let error = entry.last_error.as_deref().unwrap_or("");
                println!(
                    "{} {} status={} attempts={} {}",
                    entry.id, entry.account_email, entry.status, entry.attempts, error
                );
            }
        });
        Ok(())
    }

    pub fn outbox_retry(&self, args: OutboxRetryArgs) -> Result<()> {
        let (request_json, idempotency_key, account_email): (String, String, String) = self
            .conn
            .query_row(
                "SELECT request_json, idempotency_key, account_email FROM outbox WHERE id = ?1 AND status = 'failed'",
                params![args.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .context("outbox entry not found or not in failed state")?;

        let account = self.get_account(&account_email)?;
        let client = self.client_for_profile(&account.profile_name)?;
        let request: SendEmailRequest = serde_json::from_str(&request_json)?;

        match client.send_email(&request, &idempotency_key) {
            Ok(response) => {
                self.outbox_mark_sent(&idempotency_key)?;
                print_success_or(
                    self.format,
                    &serde_json::json!({"id": response.id, "retried": true}),
                    |_| {
                        println!("retried and sent {}", response.id);
                    },
                );
            }
            Err(err) => {
                self.outbox_mark_failed(&idempotency_key, &err.to_string())?;
                bail!("retry failed: {}", err);
            }
        }
        Ok(())
    }

    pub fn outbox_flush(&self) -> Result<()> {
        // Only pick up `pending` rows on an automatic flush. Previously this
        // swept `failed` rows back in too, which created an indefinite retry
        // loop for permanently-bad entries (deleted account, malformed JSON,
        // bad recipient). Failed rows now stay put until the user explicitly
        // retries them via `outbox retry <id>` — that path already resets
        // status back to `pending` on the way through.
        let mut stmt = self.conn.prepare(
            "SELECT id, request_json, idempotency_key, account_email FROM outbox WHERE status = 'pending' ORDER BY created_at",
        )?;

        struct PendingEntry {
            _id: String,
            request_json: String,
            idempotency_key: String,
            account_email: String,
        }

        let entries: Vec<PendingEntry> = stmt
            .query_map([], |row| {
                Ok(PendingEntry {
                    _id: row.get(0)?,
                    request_json: row.get(1)?,
                    idempotency_key: row.get(2)?,
                    account_email: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut sent = 0usize;
        let mut failed = 0usize;

        for entry in &entries {
            // Each of the three pre-flight steps below (account lookup,
            // client creation, JSON parse) used to just bump the `failed`
            // counter and `continue` without touching the DB row. That left
            // the entry in `pending` / `failed` status with no update —
            // stuck forever across restarts until manually retried, with no
            // `last_error` to explain why. Now we persist the failure via
            // `outbox_mark_failed` so `outbox list` surfaces the real
            // reason. Result ignored on purpose: we're already on the
            // failure path, double-faulting on a DB write shouldn't mask
            // the actual error (ritalin O-011; GPT-5.4 Pro Rust #3).
            let account = match self.get_account(&entry.account_email) {
                Ok(a) => a,
                Err(err) => {
                    let _ = self.outbox_mark_failed(&entry.idempotency_key, &err.to_string());
                    failed += 1;
                    continue;
                }
            };
            let client = match self.client_for_profile(&account.profile_name) {
                Ok(c) => c,
                Err(err) => {
                    let _ = self.outbox_mark_failed(&entry.idempotency_key, &err.to_string());
                    failed += 1;
                    continue;
                }
            };
            let request: SendEmailRequest = match serde_json::from_str(&entry.request_json) {
                Ok(r) => r,
                Err(err) => {
                    let _ = self.outbox_mark_failed(&entry.idempotency_key, &err.to_string());
                    failed += 1;
                    continue;
                }
            };

            match client.send_email(&request, &entry.idempotency_key) {
                Ok(_) => {
                    let _ = self.outbox_mark_sent(&entry.idempotency_key);
                    sent += 1;
                }
                Err(err) => {
                    let _ = self.outbox_mark_failed(&entry.idempotency_key, &err.to_string());
                    failed += 1;
                }
            }
        }

        print_success_or(
            self.format,
            &serde_json::json!({"sent": sent, "failed": failed}),
            |_| {
                println!("flushed: {} sent, {} failed", sent, failed);
            },
        );
        Ok(())
    }
}

fn stable_idempotency_key(request: &SendEmailRequest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request.from.as_bytes());
    let mut sorted_to = request.to.clone();
    sorted_to.sort();
    for to in &sorted_to {
        hasher.update(to.as_bytes());
    }
    // cc/bcc change the audience; without hashing them, two sends that differ
    // only by carbon-copy collide onto the same key and the second one gets
    // silently suppressed by Resend's idempotency check (ritalin O-010).
    let mut sorted_cc = request.cc.clone();
    sorted_cc.sort();
    for cc in &sorted_cc {
        hasher.update(cc.as_bytes());
    }
    let mut sorted_bcc = request.bcc.clone();
    sorted_bcc.sort();
    for bcc in &sorted_bcc {
        hasher.update(bcc.as_bytes());
    }
    hasher.update(request.subject.as_bytes());
    if let Some(text) = &request.text {
        hasher.update(text.as_bytes());
    }
    if let Some(html) = &request.html {
        hasher.update(html.as_bytes());
    }
    if let Some(headers) = &request.headers {
        let mut sorted_headers = headers.iter().collect::<Vec<_>>();
        sorted_headers.sort_by(|a, b| a.0.cmp(b.0));
        for (name, value) in sorted_headers {
            hasher.update(name.as_bytes());
            hasher.update(value.as_bytes());
        }
    }
    // Same motivation as cc/bcc: two messages with identical headers but
    // different attachments must not share an idempotency key.
    for attachment in &request.attachments {
        hasher.update(attachment.filename.as_bytes());
        hasher.update(attachment.content.as_bytes());
    }
    let hash = hasher.finalize();
    format!("email-cli-{:x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SendAttachment, SendEmailRequest};

    fn base_request() -> SendEmailRequest {
        SendEmailRequest {
            from: "a@example.com".into(),
            to: vec!["b@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "hello".into(),
            text: Some("hi".into()),
            html: None,
            headers: None,
            attachments: vec![],
        }
    }

    #[test]
    fn key_differs_when_cc_differs() {
        let r1 = base_request();
        let mut r2 = base_request();
        r2.cc = vec!["c@example.com".into()];
        assert_ne!(stable_idempotency_key(&r1), stable_idempotency_key(&r2));
    }

    #[test]
    fn key_differs_when_bcc_differs() {
        let r1 = base_request();
        let mut r2 = base_request();
        r2.bcc = vec!["d@example.com".into()];
        assert_ne!(stable_idempotency_key(&r1), stable_idempotency_key(&r2));
    }

    #[test]
    fn key_differs_when_attachment_added() {
        let r1 = base_request();
        let mut r2 = base_request();
        r2.attachments.push(SendAttachment {
            filename: "x.pdf".into(),
            content: "AAAA".into(),
        });
        assert_ne!(stable_idempotency_key(&r1), stable_idempotency_key(&r2));
    }

    #[test]
    fn key_differs_when_attachment_content_differs() {
        let mut r1 = base_request();
        r1.attachments.push(SendAttachment {
            filename: "x.pdf".into(),
            content: "AAAA".into(),
        });
        let mut r2 = base_request();
        r2.attachments.push(SendAttachment {
            filename: "x.pdf".into(),
            content: "BBBB".into(),
        });
        assert_ne!(stable_idempotency_key(&r1), stable_idempotency_key(&r2));
    }

    #[test]
    fn key_differs_when_headers_differ() {
        let r1 = base_request();
        let mut r2 = base_request();
        r2.headers = Some(std::collections::HashMap::from([(
            "In-Reply-To".into(),
            "<message-a@example.com>".into(),
        )]));
        assert_ne!(stable_idempotency_key(&r1), stable_idempotency_key(&r2));
    }

    #[test]
    fn key_stable_regardless_of_recipient_order() {
        let mut r1 = base_request();
        r1.to = vec!["a@e.com".into(), "b@e.com".into()];
        r1.cc = vec!["x@e.com".into(), "y@e.com".into()];
        r1.bcc = vec!["m@e.com".into(), "n@e.com".into()];
        let mut r2 = base_request();
        r2.to = vec!["b@e.com".into(), "a@e.com".into()];
        r2.cc = vec!["y@e.com".into(), "x@e.com".into()];
        r2.bcc = vec!["n@e.com".into(), "m@e.com".into()];
        assert_eq!(stable_idempotency_key(&r1), stable_idempotency_key(&r2));
    }

    #[test]
    fn key_matches_for_identical_requests() {
        assert_eq!(
            stable_idempotency_key(&base_request()),
            stable_idempotency_key(&base_request())
        );
    }
}
