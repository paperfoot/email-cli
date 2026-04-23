use anyhow::Result;
use rusqlite::params;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::app::App;
use crate::cli::{
    DraftCreateArgs, DraftDeleteArgs, DraftEditArgs, DraftListArgs, DraftSendArgs, DraftShowArgs,
};
use crate::helpers::{
    ensure_reply_account_matches, normalize_email, normalize_emails,
    remove_draft_attachment_snapshot, reply_headers_for_message, snapshot_draft_attachments,
    to_json,
};
use crate::models::ResolvedCompose;
use crate::output::print_success_or;

impl App {
    pub fn draft_create(&self, args: DraftCreateArgs) -> Result<()> {
        let compose = self.resolve_compose(args.compose)?;
        let id = Uuid::new_v4().to_string();
        if let Some(message_id) = args.reply_to {
            let target = self.get_message(message_id)?;
            ensure_reply_account_matches(&target, &compose.account)?;
        }
        let attachment_paths = snapshot_draft_attachments(
            self.db_path.parent().unwrap_or(Path::new(".")),
            &id,
            &compose.attachments,
        )?;
        self.conn.execute(
            "
            INSERT INTO drafts (
                id, account_email, to_json, cc_json, bcc_json, subject,
                text_body, html_body, reply_to_message_id, attachment_paths_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ",
            params![
                id,
                compose.account.email,
                to_json(&compose.to)?,
                to_json(&compose.cc)?,
                to_json(&compose.bcc)?,
                compose.subject,
                compose.text,
                compose.html,
                args.reply_to,
                to_json(&attachment_paths)?,
            ],
        )?;
        let draft = self.get_draft(&id)?;

        print_success_or(self.format, &draft, |draft| {
            println!("saved draft {}", draft.id);
        });

        Ok(())
    }

    pub fn draft_list(&self, args: DraftListArgs) -> Result<()> {
        let drafts = if let Some(account) = args.account {
            let account = normalize_email(&account);
            self.list_drafts_for_account(&account)?
        } else {
            self.list_all_drafts()?
        };

        print_success_or(self.format, &drafts, |drafts| {
            for draft in drafts {
                println!("{} {} {}", draft.id, draft.account_email, draft.subject);
            }
        });

        Ok(())
    }

    pub fn draft_show(&self, args: DraftShowArgs) -> Result<()> {
        let draft = self.get_draft(&args.id)?;

        print_success_or(self.format, &draft, |draft| {
            println!("draft {}", draft.id);
            println!("account: {}", draft.account_email);
            println!("to: {}", draft.to.join(", "));
            println!("subject: {}", draft.subject);
            if let Some(text) = &draft.text_body {
                println!();
                println!("{}", text);
            }
        });

        Ok(())
    }

    pub fn draft_send(&self, args: DraftSendArgs) -> Result<()> {
        let draft = self.get_draft(&args.id)?;
        let account = self.get_account(&draft.account_email)?;
        let reply_context = if let Some(message_id) = draft.reply_to_message_id {
            let target = self.get_message(message_id)?;
            ensure_reply_account_matches(&target, &account)?;
            Some((target.id, reply_headers_for_message(&target)))
        } else {
            None
        };
        let compose = ResolvedCompose {
            account,
            to: draft.to.clone(),
            cc: draft.cc.clone(),
            bcc: draft.bcc.clone(),
            subject: draft.subject.clone(),
            text: draft.text_body.clone(),
            html: draft.html_body.clone(),
            attachments: draft
                .attachment_paths
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>(),
        };
        let message = self.send_compose(compose, reply_context)?;
        self.conn
            .execute("DELETE FROM drafts WHERE id = ?1", params![draft.id])?;
        remove_draft_attachment_snapshot(
            self.db_path.parent().unwrap_or(Path::new(".")),
            &draft.id,
        )?;

        print_success_or(self.format, &message, |message| {
            println!("sent draft as message {}", message.id);
        });

        Ok(())
    }

    pub fn draft_edit(&self, args: DraftEditArgs) -> Result<()> {
        let draft = self.get_draft(&args.id)?;

        let subject = args.subject.unwrap_or(draft.subject);
        let text_body = args.text.or(draft.text_body);
        let html_body = args.html.or(draft.html_body);
        let to = args.to.map(|v| normalize_emails(&v)).unwrap_or(draft.to);
        let cc = args.cc.map(|v| normalize_emails(&v)).unwrap_or(draft.cc);
        let bcc = args.bcc.map(|v| normalize_emails(&v)).unwrap_or(draft.bcc);

        // Attachment handling: three mutually-exclusive states.
        //   1. `--attach <path> ...` -> replace list with freshly snapshotted copies
        //   2. `--clear-attachments` -> blow away the stored list + on-disk snapshots
        //   3. neither -> leave `attachment_paths_json` untouched so existing files survive
        // Clarity wins over cleverness here: two separate UPDATE paths beat a single
        // parameterised query with a conditional column. Option (3) is the common
        // path and must not touch the column, so we branch on `replace_attachments`.
        let replace_attachments = !args.attachments.is_empty() || args.clear_attachments;

        if replace_attachments {
            // Drop the old snapshot directory before laying down new files so a
            // shrinking attachment list doesn't leave orphaned bytes behind.
            remove_draft_attachment_snapshot(
                self.db_path.parent().unwrap_or(Path::new(".")),
                &args.id,
            )?;
            let new_paths = snapshot_draft_attachments(
                self.db_path.parent().unwrap_or(Path::new(".")),
                &args.id,
                &args.attachments,
            )?;
            self.conn.execute(
                "UPDATE drafts SET subject = ?1, text_body = ?2, html_body = ?3,
                 to_json = ?4, cc_json = ?5, bcc_json = ?6,
                 attachment_paths_json = ?7, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?8",
                params![
                    subject,
                    text_body,
                    html_body,
                    to_json(&to)?,
                    to_json(&cc)?,
                    to_json(&bcc)?,
                    to_json(&new_paths)?,
                    args.id,
                ],
            )?;
        } else {
            self.conn.execute(
                "UPDATE drafts SET subject = ?1, text_body = ?2, html_body = ?3,
                 to_json = ?4, cc_json = ?5, bcc_json = ?6, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?7",
                params![
                    subject,
                    text_body,
                    html_body,
                    to_json(&to)?,
                    to_json(&cc)?,
                    to_json(&bcc)?,
                    args.id,
                ],
            )?;
        }

        print_success_or(
            self.format,
            &serde_json::json!({"id": args.id, "updated": true}),
            |_| {
                println!("updated draft {}", args.id);
            },
        );
        Ok(())
    }

    pub fn draft_delete(&self, args: DraftDeleteArgs) -> Result<()> {
        let count = self
            .conn
            .execute("DELETE FROM drafts WHERE id = ?1", params![args.id])?;
        if count == 0 {
            anyhow::bail!("draft {} not found", args.id);
        }
        remove_draft_attachment_snapshot(
            self.db_path.parent().unwrap_or(Path::new(".")),
            &args.id,
        )?;
        print_success_or(
            self.format,
            &serde_json::json!({"id": args.id, "deleted": true}),
            |_| {
                println!("deleted draft {}", args.id);
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Build an isolated App backed by a real on-disk SQLite file inside a
    /// unique temp dir, so `snapshot_draft_attachments` has somewhere to write
    /// its draft-attachments/ tree. Not in-memory because the attachment
    /// snapshotting relies on `db_path.parent()`.
    fn test_app() -> (App, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("email-cli-test-{}", nanos));
        fs::create_dir_all(&root).unwrap();
        let db_path = root.join("email-cli.db");
        let app = App::new(db_path, Format::Json).unwrap();
        // Seed a profile + account so the drafts FK constraint is satisfied.
        app.conn
            .execute(
                "INSERT INTO profiles (name, api_key) VALUES ('default', 'test-key')",
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
        (app, root)
    }

    fn write_attachment(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    /// End-to-end: create a draft with two attachments, then `draft_edit` with
    /// a new `--attach` list containing a different file. After the edit,
    /// `get_draft` must return exactly the replacement set — verifying the
    /// attachment_paths_json column is overwritten (not appended or ignored).
    #[test]
    fn draft_edit_replaces_attachment_list() {
        let (app, root) = test_app();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let a1 = write_attachment(&src, "one.txt", b"first");
        let a2 = write_attachment(&src, "two.txt", b"second");
        let replacement = write_attachment(&src, "three.txt", b"third");

        // Seed a draft row directly so we don't need to stub Resend / compose
        // resolution. Mimics what draft_create would have written.
        let id = "draft-test-001".to_string();
        let initial = snapshot_draft_attachments(
            app.db_path.parent().unwrap(),
            &id,
            &[a1.clone(), a2.clone()],
        )
        .unwrap();
        assert_eq!(initial.len(), 2);
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json,
                    subject, text_body, html_body, reply_to_message_id,
                    attachment_paths_json)
                 VALUES (?1, 'agent@example.com', '[]', '[]', '[]', 'hi', 'body',
                    NULL, NULL, ?2)",
                params![id, to_json(&initial).unwrap()],
            )
            .unwrap();

        app.draft_edit(DraftEditArgs {
            id: id.clone(),
            subject: None,
            text: None,
            html: None,
            to: None,
            cc: None,
            bcc: None,
            attachments: vec![replacement.clone()],
            clear_attachments: false,
        })
        .unwrap();

        let reloaded = app.get_draft(&id).unwrap();
        assert_eq!(reloaded.attachment_paths.len(), 1);
        assert!(
            reloaded.attachment_paths[0].ends_with("three.txt"),
            "expected snapshot path to end with three.txt, got {}",
            reloaded.attachment_paths[0]
        );

        // Cleanup (best-effort — test isolation already ensured by unique dir).
        let _ = fs::remove_dir_all(&root);
    }

    /// --clear-attachments wipes the stored list even without --attach, and
    /// does NOT touch unrelated fields like subject.
    #[test]
    fn draft_edit_clear_attachments_empties_list() {
        let (app, root) = test_app();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let a1 = write_attachment(&src, "keep.txt", b"data");

        let id = "draft-test-002".to_string();
        let initial =
            snapshot_draft_attachments(app.db_path.parent().unwrap(), &id, &[a1]).unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json,
                    subject, text_body, html_body, reply_to_message_id,
                    attachment_paths_json)
                 VALUES (?1, 'agent@example.com', '[]', '[]', '[]', 'orig-subject',
                    NULL, NULL, NULL, ?2)",
                params![id, to_json(&initial).unwrap()],
            )
            .unwrap();

        app.draft_edit(DraftEditArgs {
            id: id.clone(),
            subject: None,
            text: None,
            html: None,
            to: None,
            cc: None,
            bcc: None,
            attachments: vec![],
            clear_attachments: true,
        })
        .unwrap();

        let reloaded = app.get_draft(&id).unwrap();
        assert!(reloaded.attachment_paths.is_empty());
        assert_eq!(reloaded.subject, "orig-subject");

        let _ = fs::remove_dir_all(&root);
    }

    /// Omitting both --attach and --clear-attachments must NOT touch the
    /// stored list — this is the common path the Swift GUI relies on when a
    /// user only edits the subject/body.
    #[test]
    fn draft_edit_without_attach_preserves_existing_list() {
        let (app, root) = test_app();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let a1 = write_attachment(&src, "survivor.txt", b"bytes");

        let id = "draft-test-003".to_string();
        let initial =
            snapshot_draft_attachments(app.db_path.parent().unwrap(), &id, &[a1]).unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json,
                    subject, text_body, html_body, reply_to_message_id,
                    attachment_paths_json)
                 VALUES (?1, 'agent@example.com', '[]', '[]', '[]', 'old',
                    NULL, NULL, NULL, ?2)",
                params![id, to_json(&initial).unwrap()],
            )
            .unwrap();

        app.draft_edit(DraftEditArgs {
            id: id.clone(),
            subject: Some("new-subject".into()),
            text: None,
            html: None,
            to: None,
            cc: None,
            bcc: None,
            attachments: vec![],
            clear_attachments: false,
        })
        .unwrap();

        let reloaded = app.get_draft(&id).unwrap();
        assert_eq!(reloaded.subject, "new-subject");
        assert_eq!(reloaded.attachment_paths.len(), 1);
        assert!(reloaded.attachment_paths[0].ends_with("survivor.txt"));

        let _ = fs::remove_dir_all(&root);
    }
}
