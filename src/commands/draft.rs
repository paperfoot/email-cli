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
    remove_draft_attachment_snapshot, reply_headers_for_message, snapshot_draft_attachments, to_json,
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
        let to = args
            .to
            .map(|v| normalize_emails(&v))
            .unwrap_or(draft.to);
        let cc = args
            .cc
            .map(|v| normalize_emails(&v))
            .unwrap_or(draft.cc);
        let bcc = args
            .bcc
            .map(|v| normalize_emails(&v))
            .unwrap_or(draft.bcc);

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
