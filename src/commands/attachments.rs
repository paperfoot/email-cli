use anyhow::{Result, anyhow, bail};
use rusqlite::params;
use serde_json::json;
use std::fs;
use std::path::Path;

use crate::app::App;
use crate::cli::{AttachmentGetArgs, AttachmentListArgs, AttachmentPrefetchArgs};
use crate::helpers::write_file_safely;
use crate::output::print_success_or;

impl App {
    pub fn attachments_list(&self, args: AttachmentListArgs) -> Result<()> {
        let message = self.get_message(args.message_id)?;
        if message.direction == "received" {
            let account = self.get_account(&message.account_email)?;
            let client = self.client_for_profile(&account.profile_name)?;
            let attachments = client.list_received_attachments(&message.remote_id)?;
            self.store_received_attachments(message.id, &attachments)?;
        }
        let rows = self.list_attachments(args.message_id)?;

        print_success_or(self.format, &rows, |rows| {
            for row in rows {
                let remote = row
                    .remote_attachment_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                let name = row
                    .filename
                    .clone()
                    .unwrap_or_else(|| "attachment".to_string());
                println!("{} {}", remote, name);
            }
        });

        Ok(())
    }

    pub fn attachments_get(&self, args: AttachmentGetArgs) -> Result<()> {
        let message = self.get_message(args.message_id)?;
        if message.direction != "received" {
            bail!("attachment download is only supported for received messages");
        }
        let account = self.get_account(&message.account_email)?;
        let client = self.client_for_profile(&account.profile_name)?;
        let attachments = client.list_received_attachments(&message.remote_id)?;
        self.store_received_attachments(message.id, &attachments)?;
        let attachment = self
            .find_attachment(args.message_id, &args.attachment_id)?
            .ok_or_else(|| anyhow!("attachment {} not found", args.attachment_id))?;
        let download_url = attachment
            .download_url
            .clone()
            .ok_or_else(|| anyhow!("attachment {} has no download url", args.attachment_id))?;
        let output_dir = args.output.unwrap_or_else(|| {
            self.db_path
                .parent()
                .unwrap_or(Path::new("."))
                .join("downloads")
        });
        fs::create_dir_all(&output_dir)?;
        let preferred_filename = attachment
            .filename
            .clone()
            .unwrap_or_else(|| format!("attachment-{}", args.attachment_id));
        let bytes = client.download_attachment(&download_url)?;
        let output_path = write_file_safely(&output_dir, &preferred_filename, &bytes)?;
        self.conn.execute(
            "UPDATE attachments SET local_path = ?1 WHERE id = ?2",
            params![output_path.display().to_string(), attachment.id],
        )?;

        let data = json!({
            "message_id": args.message_id,
            "attachment_id": args.attachment_id,
            "path": output_path.display().to_string(),
        });
        print_success_or(self.format, &data, |_d| {
            println!("{}", output_path.display());
        });

        Ok(())
    }

    /// Eagerly cache any attachment that doesn't have a local file yet. Iterates
    /// messages newest-first. For each candidate message, one Resend API call
    /// refreshes the signed URLs (they expire — see the 403s you'll otherwise
    /// hit at click time), then each attachment is downloaded and `local_path`
    /// is persisted. Failures are counted but don't abort the run — Minimail
    /// fires this after every sync, so transient errors heal on the next tick.
    pub fn attachments_prefetch(&self, args: AttachmentPrefetchArgs) -> Result<()> {
        // Step 1 — enumerate candidate messages (one message may have multiple
        // attachments; we dedupe so we only hit Resend's list endpoint once per
        // message).
        let mut candidates: Vec<(i64, String, String)> = Vec::new();
        if let Some(ref account) = args.account {
            let acct = crate::helpers::normalize_email(account);
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT a.message_id, m.remote_id, m.account_email
                 FROM attachments a
                 JOIN messages m ON a.message_id = m.id
                 WHERE a.local_path IS NULL
                   AND m.direction = 'received'
                   AND m.account_email = ?1
                 ORDER BY m.created_at DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![acct, args.limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                candidates.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT a.message_id, m.remote_id, m.account_email
                 FROM attachments a
                 JOIN messages m ON a.message_id = m.id
                 WHERE a.local_path IS NULL
                   AND m.direction = 'received'
                 ORDER BY m.created_at DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![args.limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                candidates.push(row?);
            }
        }

        let output_dir = self
            .db_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("downloads");
        fs::create_dir_all(&output_dir)?;

        let mut downloaded = 0usize;
        let mut errors = 0usize;

        for (message_id, remote_id, account_email) in candidates {
            let account = match self.get_account(&account_email) {
                Ok(a) => a,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };
            let client = match self.client_for_profile(&account.profile_name) {
                Ok(c) => c,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };

            // Refresh URLs — Resend's signed download links expire; re-fetching
            // from /emails/receiving/{id}/attachments yields fresh ones.
            let fresh = match client.list_received_attachments(&remote_id) {
                Ok(list) => list,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };
            if self.store_received_attachments(message_id, &fresh).is_err() {
                errors += 1;
                continue;
            }

            // Re-read the local rows so we get current (filename, local_path,
            // freshly-updated download_url).
            let rows = match self.list_attachments(message_id) {
                Ok(r) => r,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };
            for attachment in rows {
                if attachment.local_path.is_some() {
                    continue;
                }
                let Some(url) = attachment.download_url.as_deref() else {
                    // Resend gave us no URL even after the refresh — skip
                    // quietly. This happens for inline images embedded via CID
                    // that aren't exposed as separate downloadable files.
                    continue;
                };
                let filename = attachment
                    .filename
                    .clone()
                    .unwrap_or_else(|| format!("attachment-{}", attachment.id));
                let bytes = match client.download_attachment(url) {
                    Ok(b) => b,
                    Err(_) => {
                        errors += 1;
                        continue;
                    }
                };
                let output_path = match write_file_safely(&output_dir, &filename, &bytes) {
                    Ok(p) => p,
                    Err(_) => {
                        errors += 1;
                        continue;
                    }
                };
                self.conn.execute(
                    "UPDATE attachments SET local_path = ?1 WHERE id = ?2",
                    params![output_path.display().to_string(), attachment.id],
                )?;
                downloaded += 1;
            }
        }

        let data = json!({
            "downloaded": downloaded,
            "errors": errors,
        });
        print_success_or(self.format, &data, |_d| {
            if downloaded == 0 && errors == 0 {
                println!("no attachments to prefetch");
            } else {
                println!(
                    "prefetched {} attachment(s); {} error(s)",
                    downloaded, errors
                );
            }
        });
        Ok(())
    }
}
