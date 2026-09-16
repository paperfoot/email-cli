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
    remove_draft_attachment_snapshot, remove_unreferenced_draft_attachments,
    reply_headers_for_message, snapshot_draft_attachments, to_json,
};
use crate::models::{DraftRecord, ResolvedCompose};
use crate::output::print_success_or;

#[derive(serde::Serialize)]
struct DraftEditResponse {
    #[serde(flatten)]
    draft: DraftRecord,
    updated: bool,
}

impl App {
    pub fn draft_create(&self, args: DraftCreateArgs) -> Result<()> {
        let reply_to_message_id = args.compose.reply_to_msg;
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
        let insert_result = self.conn.execute(
            "
            INSERT INTO drafts (
                id, account_email, to_json, cc_json, bcc_json, reply_to_json, subject,
                text_body, html_body, reply_to_message_id, scheduled_at, attachment_paths_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ",
            params![
                id,
                compose.account.email,
                to_json(&compose.to)?,
                to_json(&compose.cc)?,
                to_json(&compose.bcc)?,
                to_json(&compose.reply_to)?,
                compose.subject,
                compose.text,
                compose.html,
                reply_to_message_id,
                compose.scheduled_at,
                to_json(&attachment_paths)?,
            ],
        );
        if let Err(error) = insert_result {
            remove_unreferenced_draft_attachments(
                self.db_path.parent().unwrap_or(Path::new(".")),
                &id,
                &attachment_paths,
                &[],
            )?;
            return Err(error.into());
        }
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
            reply_to: draft.reply_to.clone(),
            subject: draft.subject.clone(),
            text: draft.text_body.clone(),
            html: draft.html_body.clone(),
            attachments: draft
                .attachment_paths
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>(),
            scheduled_at: draft.scheduled_at.clone(),
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
        let html_body = if args.clear_html {
            None
        } else {
            args.html.or(draft.html_body)
        };
        let to = args.to.map(|v| normalize_emails(&v)).unwrap_or(draft.to);
        let cc = args.cc.map(|v| normalize_emails(&v)).unwrap_or(draft.cc);
        let bcc = args.bcc.map(|v| normalize_emails(&v)).unwrap_or(draft.bcc);
        let reply_to = args
            .reply_to
            .map(|v| normalize_emails(&v))
            .unwrap_or(draft.reply_to);
        let scheduled_at = if args.clear_schedule {
            None
        } else if let Some(value) = args.scheduled_at {
            Some(value.trim().to_string()).filter(|value| !value.is_empty())
        } else {
            draft.scheduled_at
        };
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
        // New files are copied into a unique revision before the DB changes.
        // This is deliberate: GUI edits commonly pass the draft's existing
        // snapshot paths back as sources, so deleting the old directory first
        // destroys the only copy before it can be read.
        let replace_attachments = !args.attachments.is_empty() || args.clear_attachments;
        let base_dir = self.db_path.parent().unwrap_or(Path::new("."));
        let old_attachment_paths = draft.attachment_paths;

        if replace_attachments {
            let new_paths = if args.attachments.is_empty() {
                Vec::new()
            } else {
                snapshot_draft_attachments(base_dir, &args.id, &args.attachments)?
            };
            let new_paths_json = match to_json(&new_paths) {
                Ok(json) => json,
                Err(error) => {
                    remove_unreferenced_draft_attachments(base_dir, &args.id, &new_paths, &[])?;
                    return Err(error);
                }
            };
            let update_result = self.conn.execute(
                "UPDATE drafts SET account_email = ?1, subject = ?2, text_body = ?3, html_body = ?4,
                 to_json = ?5, cc_json = ?6, bcc_json = ?7, reply_to_json = ?8,
                 scheduled_at = ?9, attachment_paths_json = ?10, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?11",
                params![
                    account_email,
                    subject,
                    text_body,
                    html_body,
                    to_json(&to)?,
                    to_json(&cc)?,
                    to_json(&bcc)?,
                    to_json(&reply_to)?,
                    scheduled_at,
                    new_paths_json,
                    args.id,
                ],
            );
            if let Err(error) = update_result {
                remove_unreferenced_draft_attachments(base_dir, &args.id, &new_paths, &[])?;
                return Err(error.into());
            }
            // The new row is committed. A cleanup failure must not report the
            // save as failed and leave callers holding now-obsolete paths.
            if let Err(error) = remove_unreferenced_draft_attachments(
                base_dir,
                &args.id,
                &old_attachment_paths,
                &new_paths,
            ) {
                eprintln!("warning: draft saved; old attachment cleanup failed: {error}");
            }
        } else {
            self.conn.execute(
                "UPDATE drafts SET account_email = ?1, subject = ?2, text_body = ?3, html_body = ?4,
                 to_json = ?5, cc_json = ?6, bcc_json = ?7, reply_to_json = ?8,
                 scheduled_at = ?9, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?10",
                params![
                    account_email,
                    subject,
                    text_body,
                    html_body,
                    to_json(&to)?,
                    to_json(&cc)?,
                    to_json(&bcc)?,
                    to_json(&reply_to)?,
                    scheduled_at,
                    args.id,
                ],
            )?;
        }

        let response = DraftEditResponse {
            draft: self.get_draft(&args.id)?,
            updated: true,
        };
        print_success_or(self.format, &response, |response| {
            println!("updated draft {}", response.draft.id);
        });
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
            reply_to_header: vec![],
            scheduled_at: None,
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

        app.draft_create(DraftCreateArgs { compose }).unwrap();

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

        app.draft_create(DraftCreateArgs { compose }).unwrap();

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
            clear_html: false,
            to: Some(vec![String::new()]),
            cc: Some(vec![String::new()]),
            bcc: Some(vec![String::new()]),
            reply_to: None,
            scheduled_at: None,
            clear_schedule: false,
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
            clear_html: false,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            scheduled_at: None,
            clear_schedule: false,
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
            clear_html: false,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            scheduled_at: None,
            clear_schedule: false,
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
            clear_html: false,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            scheduled_at: None,
            clear_schedule: false,
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

    #[test]
    fn draft_metadata_round_trips_and_can_be_cleared() {
        let (app, root) = test_app();
        let mut compose = empty_compose();
        compose.reply_to_header = vec!["Support <support@example.com>".into()];
        compose.scheduled_at = Some("2026-12-01T09:30:00Z".into());
        compose.html = Some("<p>Rich</p>".into());

        app.draft_create(DraftCreateArgs { compose }).unwrap();
        let created = app.list_all_drafts().unwrap().remove(0);
        assert_eq!(created.reply_to, vec!["support@example.com"]);
        assert_eq!(
            created.scheduled_at.as_deref(),
            Some("2026-12-01T09:30:00Z")
        );
        assert_eq!(created.html_body.as_deref(), Some("<p>Rich</p>"));

        app.draft_edit(DraftEditArgs {
            id: created.id.clone(),
            subject: None,
            text: None,
            html: None,
            clear_html: true,
            to: None,
            cc: None,
            bcc: None,
            reply_to: Some(vec![String::new()]),
            scheduled_at: None,
            clear_schedule: true,
            account: None,
            attachments: vec![],
            clear_attachments: false,
        })
        .unwrap();

        let cleared = app.get_draft(&created.id).unwrap();
        assert!(cleared.reply_to.is_empty());
        assert!(cleared.scheduled_at.is_none());
        assert!(cleared.html_body.is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_edit_response_contains_the_full_persisted_draft() {
        let (app, root) = test_app();
        let id = "draft-edit-response".to_string();
        app.conn
            .execute(
                "INSERT INTO drafts (
                    id, account_email, to_json, reply_to_json, subject, text_body,
                    scheduled_at, attachment_paths_json
                 ) VALUES (
                    ?1, 'agent@example.com', '[\"to@example.com\"]',
                    '[\"reply@example.com\"]', 'subject', 'body',
                    '2026-12-01T09:30:00Z', '[\"/tmp/attached.txt\"]'
                 )",
                params![id],
            )
            .unwrap();

        let response = DraftEditResponse {
            draft: app.get_draft(&id).unwrap(),
            updated: true,
        };
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["id"], id);
        assert_eq!(json["updated"], true);
        assert_eq!(json["to"], serde_json::json!(["to@example.com"]));
        assert_eq!(json["reply_to"], serde_json::json!(["reply@example.com"]));
        assert_eq!(json["scheduled_at"], "2026-12-01T09:30:00Z");
        assert_eq!(
            json["attachment_paths"],
            serde_json::json!(["/tmp/attached.txt"])
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn repeated_edit_can_snapshot_its_existing_attachment() {
        let (app, root) = test_app();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let source = write_attachment(&src, "repeat.txt", b"survives repeated autosaves");
        let id = "draft-repeat-snapshot".to_string();
        let initial =
            snapshot_draft_attachments(app.db_path.parent().unwrap(), &id, &[source]).unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, attachment_paths_json)
                 VALUES (?1, 'agent@example.com', '[]', ?2)",
                params![id, to_json(&initial).unwrap()],
            )
            .unwrap();

        for _ in 0..2 {
            let before = app.get_draft(&id).unwrap().attachment_paths[0].clone();
            app.draft_edit(DraftEditArgs {
                id: id.clone(),
                subject: None,
                text: None,
                html: None,
                clear_html: false,
                to: None,
                cc: None,
                bcc: None,
                reply_to: None,
                scheduled_at: None,
                clear_schedule: false,
                account: None,
                attachments: vec![PathBuf::from(&before)],
                clear_attachments: false,
            })
            .unwrap();
            let after = app.get_draft(&id).unwrap().attachment_paths[0].clone();
            assert_ne!(before, after);
            assert!(!Path::new(&before).exists());
            assert_eq!(fs::read(after).unwrap(), b"survives repeated autosaves");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_attachment_replacement_preserves_old_snapshot_and_db_reference() {
        let (app, root) = test_app();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let source = write_attachment(&src, "original.txt", b"original bytes");
        let id = "draft-failed-replacement".to_string();
        let initial =
            snapshot_draft_attachments(app.db_path.parent().unwrap(), &id, &[source]).unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, attachment_paths_json)
                 VALUES (?1, 'agent@example.com', '[]', ?2)",
                params![id, to_json(&initial).unwrap()],
            )
            .unwrap();

        let old_path = initial[0].clone();
        let missing = root.join("does-not-exist.txt");
        let result = app.draft_edit(DraftEditArgs {
            id: id.clone(),
            subject: Some("must not commit".into()),
            text: None,
            html: None,
            clear_html: false,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            scheduled_at: None,
            clear_schedule: false,
            account: None,
            attachments: vec![PathBuf::from(&old_path), missing],
            clear_attachments: false,
        });
        assert!(result.is_err());

        let unchanged = app.get_draft(&id).unwrap();
        assert_eq!(unchanged.subject, "");
        assert_eq!(unchanged.attachment_paths, vec![old_path.clone()]);
        assert_eq!(fs::read(old_path).unwrap(), b"original bytes");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_db_update_removes_staging_but_preserves_old_snapshot() {
        let (app, root) = test_app();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let original = write_attachment(&src, "original.txt", b"original");
        let replacement = write_attachment(&src, "replacement.txt", b"replacement");
        let id = "draft-failed-db-update".to_string();
        let initial =
            snapshot_draft_attachments(app.db_path.parent().unwrap(), &id, &[original]).unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, attachment_paths_json)
                 VALUES (?1, 'agent@example.com', '[]', ?2)",
                params![id, to_json(&initial).unwrap()],
            )
            .unwrap();
        app.conn
            .execute_batch(
                "CREATE TRIGGER reject_draft_update BEFORE UPDATE ON drafts
                 BEGIN SELECT RAISE(FAIL, 'injected update failure'); END;",
            )
            .unwrap();

        let result = app.draft_edit(DraftEditArgs {
            id: id.clone(),
            subject: Some("must not commit".into()),
            text: None,
            html: None,
            clear_html: false,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            scheduled_at: None,
            clear_schedule: false,
            account: None,
            attachments: vec![replacement],
            clear_attachments: false,
        });
        assert!(result.is_err());

        let unchanged = app.get_draft(&id).unwrap();
        assert_eq!(unchanged.subject, "");
        assert_eq!(unchanged.attachment_paths, initial);
        assert_eq!(
            fs::read(&unchanged.attachment_paths[0]).unwrap(),
            b"original"
        );
        let draft_dir =
            crate::helpers::draft_attachment_root(app.db_path.parent().unwrap()).join(&id);
        assert_eq!(fs::read_dir(draft_dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}
