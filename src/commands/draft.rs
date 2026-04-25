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
        let reply_to_message_id = args.reply_to.or(args.compose.reply_to_msg);
        let compose = self.resolve_compose_without_body_requirement(args.compose)?;
        let id = Uuid::new_v4().to_string();
        if let Some(message_id) = reply_to_message_id {
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
                reply_to_message_id,
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
        // `account_email` is the identity the draft will send from. Only
        // touch it when Minimail explicitly provides a new one; otherwise
        // preserve whatever was stored on create so unrelated edits don't
        // silently migrate the draft to another account.
        let account_email = args
            .account
            .map(|a| crate::helpers::normalize_email(&a))
            .unwrap_or(draft.account_email);

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
                "UPDATE drafts SET account_email = ?1, subject = ?2, text_body = ?3, html_body = ?4,
                 to_json = ?5, cc_json = ?6, bcc_json = ?7,
                 attachment_paths_json = ?8, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?9",
                params![
                    account_email,
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
                "UPDATE drafts SET account_email = ?1, subject = ?2, text_body = ?3, html_body = ?4,
                 to_json = ?5, cc_json = ?6, bcc_json = ?7, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?8",
                params![
                    account_email,
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
    use crate::cli::ComposeArgs;
    use crate::output::Format;
    use std::fs;

    /// Build an isolated App backed by a real on-disk SQLite file inside a
    /// unique temp dir, so `snapshot_draft_attachments` has somewhere to write
    /// its draft-attachments/ tree. Not in-memory because the attachment
    /// snapshotting relies on `db_path.parent()`.
    fn test_app() -> (App, PathBuf) {
        let root = std::env::temp_dir().join(format!("email-cli-test-{}", Uuid::new_v4()));
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

    fn empty_compose() -> ComposeArgs {
        ComposeArgs {
            account: Some("agent@example.com".into()),
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: String::new(),
            reply_to_msg: None,
            text: None,
            text_file: None,
            html: None,
            html_file: None,
            attachments: vec![],
        }
    }

    fn write_attachment(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    fn seed_message(app: &App, id: i64) {
        app.conn
            .execute(
                "INSERT INTO messages (
                    id, remote_id, direction, account_email, from_addr, to_json, cc_json,
                    bcc_json, reply_to_json, subject, created_at, raw_json
                 ) VALUES (
                    ?1, 'remote-reply-test', 'received', 'agent@example.com',
                    'sender@example.com', '[\"agent@example.com\"]', '[]', '[]',
                    '[]', 'hello', '2026-01-01T00:00:00Z', '{}'
                 )",
                params![id],
            )
            .unwrap();
    }

    #[test]
    fn draft_create_keeps_reply_to_msg_threading() {
        let (app, root) = test_app();
        seed_message(&app, 123);
        let mut compose = empty_compose();
        compose.reply_to_msg = Some(123);
        compose.text = Some("reply body".into());

        app.draft_create(DraftCreateArgs {
            compose,
            reply_to: None,
        })
        .unwrap();

        let drafts = app.list_all_drafts().unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].reply_to_message_id, Some(123));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_create_allows_partial_autosave_states() {
        let (app, root) = test_app();
        let mut compose = empty_compose();
        compose.subject = "only a subject so far".into();

        app.draft_create(DraftCreateArgs {
            compose,
            reply_to: None,
        })
        .unwrap();

        let drafts = app.list_all_drafts().unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].subject, "only a subject so far");
        assert!(drafts[0].to.is_empty());
        assert!(drafts[0].text_body.is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_edit_can_clear_recipients_and_body() {
        let (app, root) = test_app();
        let id = "draft-clear-fields".to_string();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json,
                    subject, text_body, html_body, reply_to_message_id,
                    attachment_paths_json)
                 VALUES (?1, 'agent@example.com', '[\"old@example.com\"]',
                    '[\"cc@example.com\"]', '[\"bcc@example.com\"]', 'hi', 'old body',
                    NULL, NULL, '[]')",
                params![id],
            )
            .unwrap();

        app.draft_edit(DraftEditArgs {
            id: id.clone(),
            subject: Some(String::new()),
            text: Some(String::new()),
            html: None,
            to: Some(vec![String::new()]),
            cc: Some(vec![String::new()]),
            bcc: Some(vec![String::new()]),
            account: None,
            attachments: vec![],
            clear_attachments: false,
        })
        .unwrap();

        let reloaded = app.get_draft(&id).unwrap();
        assert!(reloaded.to.is_empty());
        assert!(reloaded.cc.is_empty());
        assert!(reloaded.bcc.is_empty());
        assert_eq!(reloaded.subject, "");
        assert_eq!(reloaded.text_body.as_deref(), Some(""));
        let _ = fs::remove_dir_all(&root);
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
            account: None,
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
            account: None,
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
            account: None,
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
