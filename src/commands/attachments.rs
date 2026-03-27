use anyhow::{Result, anyhow, bail};
use rusqlite::params;
use serde_json::json;
use std::fs;
use std::path::Path;

use crate::app::App;
use crate::cli::{AttachmentGetArgs, AttachmentListArgs};
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
}
