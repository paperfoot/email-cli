use anyhow::{Context, Result, anyhow, bail};
use rusqlite::params;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app::App;
use crate::cli::{AttachmentGetArgs, AttachmentListArgs, AttachmentPrefetchArgs};
use crate::helpers::write_file_safely;
use crate::output::print_success_or;

impl App {
    pub fn attachments_list(&self, args: AttachmentListArgs) -> Result<()> {
        let message = self.get_message(args.message_id)?;
        if let Err(err) = self.refresh_attachment_metadata(&message) {
            if self.list_attachments(args.message_id)?.is_empty() {
                return Err(err);
            }
        }
        let rows = self
            .list_attachments(args.message_id)?
            .into_iter()
            .map(|row| row.into_view())
            .collect::<Vec<_>>();

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
        let mut attachment = self.find_attachment(args.message_id, &args.attachment_id)?;
        let has_local_bytes = attachment
            .as_ref()
            .and_then(|row| row.local_path.as_deref())
            .is_some_and(|path| Path::new(path).is_file());
        if !has_local_bytes {
            self.refresh_attachment_metadata(&message)?;
            attachment = self.find_attachment(args.message_id, &args.attachment_id)?;
        }
        let attachment =
            attachment.ok_or_else(|| anyhow!("attachment {} not found", args.attachment_id))?;
        let preferred_filename = attachment
            .filename
            .clone()
            .unwrap_or_else(|| format!("attachment-{}", args.attachment_id));
        let bytes = if let Some(local_path) = attachment.local_path.as_deref()
            && Path::new(local_path).is_file()
        {
            fs::read(local_path).with_context(|| format!("failed to read cached {local_path}"))?
        } else {
            let account = self.get_account(&message.account_email)?;
            let client = self.client_for_profile(&account.profile_name)?;
            let download_url = attachment
                .download_url
                .clone()
                .ok_or_else(|| anyhow!("attachment {} has no download url", args.attachment_id))?;
            client.download_attachment(&download_url)?
        };
        // Keep the database pointed at an app-owned copy. Caller-selected
        // export files are disposable: users can move or edit them without
        // mutating the bytes future opens use.
        self.persist_canonical_attachment(&attachment, &preferred_filename, &bytes)?;
        let default_export_dir = self
            .db_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("downloads");
        let output_path =
            write_attachment_output(&args, &default_export_dir, &preferred_filename, &bytes)?;

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

    fn persist_canonical_attachment(
        &self,
        attachment: &crate::models::AttachmentRecord,
        preferred_filename: &str,
        bytes: &[u8],
    ) -> Result<PathBuf> {
        let cache_dir = self
            .db_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("attachment-cache");
        let existing_path = attachment.local_path.as_deref().map(Path::new);
        let cache_path = if existing_path.is_some_and(|path| {
            path.is_file() && path.parent().is_some_and(|parent| parent == cache_dir)
        }) {
            existing_path.expect("checked above").to_path_buf()
        } else {
            write_canonical_attachment(&cache_dir, preferred_filename, bytes)?
        };
        let stored_path = cache_path.display().to_string();

        if attachment.local_path.as_deref() != Some(stored_path.as_str()) {
            self.conn.execute(
                "UPDATE attachments SET local_path = ?1 WHERE id = ?2",
                params![stored_path, attachment.id],
            )?;
        }
        Ok(cache_path)
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
        let mut candidates: Vec<(i64, String, String, String)> = Vec::new();
        if let Some(ref account) = args.account {
            let acct = crate::helpers::normalize_email(account);
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT a.message_id, m.remote_id, m.account_email, m.direction
                 FROM attachments a
                 JOIN messages m ON a.message_id = m.id
                 WHERE a.local_path IS NULL
                   AND m.direction IN ('received', 'sent')
                   AND m.account_email = ?1
                 ORDER BY m.created_at DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![acct, args.limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            for row in rows {
                candidates.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT a.message_id, m.remote_id, m.account_email, m.direction
                 FROM attachments a
                 JOIN messages m ON a.message_id = m.id
                 WHERE a.local_path IS NULL
                   AND m.direction IN ('received', 'sent')
                 ORDER BY m.created_at DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![args.limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
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
            .join("attachment-cache");
        fs::create_dir_all(&output_dir)?;

        let mut downloaded = 0usize;
        let mut errors = 0usize;

        for (message_id, remote_id, account_email, direction) in candidates {
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
            // from the relevant attachments endpoint yields fresh ones.
            let fresh = match self.fetch_attachment_metadata(&client, &direction, &remote_id) {
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

    fn refresh_attachment_metadata(&self, message: &crate::models::MessageRecord) -> Result<()> {
        let account = self.get_account(&message.account_email)?;
        let client = self.client_for_profile(&account.profile_name)?;
        let attachments =
            self.fetch_attachment_metadata(&client, &message.direction, &message.remote_id)?;
        self.store_received_attachments(message.id, &attachments)?;
        Ok(())
    }

    fn fetch_attachment_metadata(
        &self,
        client: &crate::resend::ResendClient,
        direction: &str,
        remote_id: &str,
    ) -> Result<Vec<crate::models::ReceivedAttachment>> {
        match direction {
            "received" => client.list_received_attachments(remote_id),
            "sent" => client.list_sent_attachments(remote_id),
            other => bail!("attachments are not supported for {other} messages"),
        }
    }
}

fn write_canonical_attachment(
    cache_dir: &Path,
    preferred_filename: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    fs::create_dir_all(cache_dir)?;
    write_file_safely(cache_dir, preferred_filename, bytes)
}

fn write_attachment_output(
    args: &AttachmentGetArgs,
    default_dir: &Path,
    preferred_filename: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    if let Some(path) = args.output_file.as_deref() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
        return Ok(path.to_path_buf());
    }

    let output_dir = args
        .output_dir
        .as_deref()
        .or(args.output.as_deref())
        .unwrap_or(default_dir);
    fs::create_dir_all(output_dir)?;
    write_file_safely(output_dir, preferred_filename, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::output::Format;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "email-cli-attachments-test-{}-{name}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn output_file_writes_exact_path() {
        let dir = temp_path("file");
        let target = dir.join("renamed.pdf");
        let args = AttachmentGetArgs {
            message_id: 1,
            attachment_id: "att".to_string(),
            output: None,
            output_dir: None,
            output_file: Some(target.clone()),
        };

        let written =
            write_attachment_output(&args, &dir.join("default"), "original.pdf", b"pdf").unwrap();

        assert_eq!(written, target);
        assert_eq!(std::fs::read(&target).unwrap(), b"pdf");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_output_writes_inside_directory() {
        let dir = temp_path("dir");
        let args = AttachmentGetArgs {
            message_id: 1,
            attachment_id: "att".to_string(),
            output: Some(dir.clone()),
            output_dir: None,
            output_file: None,
        };

        let written =
            write_attachment_output(&args, &dir.join("default"), "original.pdf", b"pdf").unwrap();

        assert_eq!(written, dir.join("original.pdf"));
        assert_eq!(std::fs::read(&written).unwrap(), b"pdf");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn test_app(root: &Path) -> App {
        let app = App::new(root.join("email-cli.db"), Format::Json).unwrap();
        app.conn
            .execute(
                "INSERT INTO profiles (name, api_key) VALUES ('default', 'test')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO accounts (email, profile_name, is_default)
                 VALUES ('agent@example.com', 'default', 1)",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO messages (
                    remote_id, direction, account_email, from_addr, to_json,
                    created_at, raw_json
                 ) VALUES ('message-1', 'received', 'agent@example.com',
                    'sender@example.com', '[]', CURRENT_TIMESTAMP, '{}')",
                [],
            )
            .unwrap();
        app
    }

    fn seed_attachment(app: &App, filename: &str, local_path: Option<&Path>) -> i64 {
        app.conn
            .execute(
                "INSERT INTO attachments (
                    message_id, remote_attachment_id, filename, local_path, raw_json
                 ) VALUES (1, 'attachment-1', ?1, ?2, '{}')",
                params![filename, local_path.map(|path| path.display().to_string())],
            )
            .unwrap();
        app.conn.last_insert_rowid()
    }

    #[test]
    fn exported_copy_edits_do_not_replace_canonical_bytes() {
        let root = temp_path("owned-cache");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("caller-owned-source.pdf");
        let export = root.join("export.pdf");
        fs::write(&source, b"original").unwrap();
        let app = test_app(&root);
        let row_id = seed_attachment(&app, "report.pdf", Some(&source));
        let args = AttachmentGetArgs {
            message_id: 1,
            attachment_id: "attachment-1".to_string(),
            output: None,
            output_dir: None,
            output_file: Some(export.clone()),
        };

        app.attachments_get(args).unwrap();
        let cached: String = app
            .conn
            .query_row(
                "SELECT local_path FROM attachments WHERE id = ?1",
                [row_id],
                |row| row.get(0),
            )
            .unwrap();
        let cached = PathBuf::from(cached);
        assert_eq!(
            cached.parent(),
            Some(root.join("attachment-cache").as_path())
        );
        assert_ne!(cached, export);
        assert_eq!(fs::read(&cached).unwrap(), b"original");

        fs::write(&export, b"edited by caller").unwrap();
        app.attachments_get(AttachmentGetArgs {
            message_id: 1,
            attachment_id: "attachment-1".to_string(),
            output: None,
            output_dir: None,
            output_file: Some(export.clone()),
        })
        .unwrap();

        assert_eq!(fs::read(&export).unwrap(), b"original");
        assert_eq!(fs::read(&cached).unwrap(), b"original");
        drop(app);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_cache_pointer_is_repaired_to_canonical_storage() {
        let root = temp_path("missing-cache");
        fs::create_dir_all(&root).unwrap();
        let missing = root.join("missing.pdf");
        let app = test_app(&root);
        let row_id = seed_attachment(&app, "report.pdf", Some(&missing));
        let attachment = app.find_attachment(1, "attachment-1").unwrap().unwrap();

        let cached = app
            .persist_canonical_attachment(&attachment, "report.pdf", b"downloaded")
            .unwrap();
        let stored: String = app
            .conn
            .query_row(
                "SELECT local_path FROM attachments WHERE id = ?1",
                [row_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(PathBuf::from(stored), cached);
        assert_eq!(
            cached.parent(),
            Some(root.join("attachment-cache").as_path())
        );
        assert_eq!(fs::read(cached).unwrap(), b"downloaded");
        drop(app);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_cache_disambiguates_matching_filenames() {
        let root = temp_path("duplicate-name");
        let cache = root.join("attachment-cache");
        let first = write_canonical_attachment(&cache, "report.pdf", b"first").unwrap();
        let second = write_canonical_attachment(&cache, "report.pdf", b"second").unwrap();

        assert_ne!(first, second);
        assert_eq!(first.file_name().unwrap(), "report.pdf");
        assert_eq!(second.file_name().unwrap(), "report-1.pdf");
        assert_eq!(fs::read(first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"second");
        let _ = fs::remove_dir_all(root);
    }
}
