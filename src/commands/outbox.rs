use anyhow::{bail, Context, Result};
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
        let request_json = serde_json::to_string(request)?;
        let idempotency_key = stable_idempotency_key(request);
        let id = Uuid::new_v4().to_string();

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
        let mut stmt = self.conn.prepare(
            "SELECT id, request_json, idempotency_key, account_email FROM outbox WHERE status IN ('pending', 'failed') ORDER BY created_at",
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
            let account = match self.get_account(&entry.account_email) {
                Ok(a) => a,
                Err(_) => {
                    failed += 1;
                    continue;
                }
            };
            let client = match self.client_for_profile(&account.profile_name) {
                Ok(c) => c,
                Err(_) => {
                    failed += 1;
                    continue;
                }
            };
            let request: SendEmailRequest = match serde_json::from_str(&entry.request_json) {
                Ok(r) => r,
                Err(_) => {
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
    hasher.update(request.subject.as_bytes());
    if let Some(text) = &request.text {
        hasher.update(text.as_bytes());
    }
    if let Some(html) = &request.html {
        hasher.update(html.as_bytes());
    }
    let hash = hasher.finalize();
    format!("email-cli-{:x}", hash)
}
