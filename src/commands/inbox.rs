use anyhow::Result;
use rusqlite::params;

use crate::app::App;
use crate::cli::{InboxListArgs, InboxReadArgs};
use crate::helpers::compact_targets;
use crate::output::print_success_or;

impl App {
    pub fn inbox_list(&self, args: InboxListArgs) -> Result<()> {
        let messages = self.list_messages(args.account.as_deref(), args.limit, args.unread)?;

        print_success_or(self.format, &messages, |messages| {
            for message in messages {
                let read_flag = if message.is_read { " " } else { "*" };
                println!(
                    "{}{} [{}] {} -> {} | {}",
                    message.id,
                    read_flag,
                    message.direction,
                    message.account_email,
                    compact_targets(&message.to),
                    message.subject
                );
            }
        });

        Ok(())
    }

    pub fn inbox_read(&self, args: InboxReadArgs) -> Result<()> {
        if args.mark_read {
            self.conn.execute(
                "UPDATE messages SET is_read = 1 WHERE id = ?1",
                params![args.id],
            )?;
        }
        let message = self.get_message(args.id)?;

        print_success_or(self.format, &message, |message| {
            println!("id: {}", message.id);
            println!("account: {}", message.account_email);
            println!("direction: {}", message.direction);
            println!("from: {}", message.from_addr);
            println!("to: {}", message.to.join(", "));
            println!("subject: {}", message.subject);
            if let Some(rfc) = message.rfc_message_id.as_deref() {
                println!("message-id: {}", rfc);
            }
            println!();
            if let Some(text) = message.text_body.as_deref() {
                println!("{}", text);
            } else if let Some(html) = message.html_body.as_deref() {
                println!("{}", html);
            }
        });

        Ok(())
    }
}
