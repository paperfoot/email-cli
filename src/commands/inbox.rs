use anyhow::{Result, bail};
use rusqlite::params;

use crate::app::App;
use crate::cli::{InboxListArgs, InboxReadArgs, InboxDeleteArgs, InboxArchiveArgs, InboxSearchArgs, InboxPurgeArgs};
use crate::helpers::compact_targets;
use crate::output::print_success_or;

fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() || next == '~' {
                        break;
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    let mut prev_blank = false;
    let mut cleaned = String::new();
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank {
                cleaned.push('\n');
                prev_blank = true;
            }
        } else {
            cleaned.push_str(trimmed);
            cleaned.push('\n');
            prev_blank = false;
        }
    }
    cleaned
}

impl App {
    pub fn inbox_list(&self, args: InboxListArgs) -> Result<()> {
        let archived_val: i64 = if args.archived { 1 } else { 0 };
        let (sql, has_account) = match (args.account.as_deref(), args.unread) {
            (Some(_), true) => (
                "SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                        reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                        references_json, last_event, is_read, created_at, synced_at
                 FROM messages
                 WHERE account_email = ?1 AND is_read = 0 AND archived = ?2
                 ORDER BY created_at DESC
                 LIMIT ?3",
                true,
            ),
            (Some(_), false) => (
                "SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                        reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                        references_json, last_event, is_read, created_at, synced_at
                 FROM messages
                 WHERE account_email = ?1 AND archived = ?2
                 ORDER BY created_at DESC
                 LIMIT ?3",
                true,
            ),
            (None, true) => (
                "SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                        reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                        references_json, last_event, is_read, created_at, synced_at
                 FROM messages
                 WHERE is_read = 0 AND archived = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
                false,
            ),
            (None, false) => (
                "SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                        reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                        references_json, last_event, is_read, created_at, synced_at
                 FROM messages
                 WHERE archived = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
                false,
            ),
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if has_account {
            stmt.query_map(
                params![crate::helpers::normalize_email(args.account.as_deref().unwrap()), archived_val, args.limit as i64],
                crate::db::map_message,
            )?
        } else {
            stmt.query_map(params![archived_val, args.limit as i64], crate::db::map_message)?
        };
        let messages: Vec<_> = rows.collect::<std::result::Result<Vec<_>, _>>()?;

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
        let raw = args.raw;
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
                if raw {
                    println!("{}", text);
                } else {
                    println!("{}", strip_ansi(text));
                }
            } else if let Some(html) = message.html_body.as_deref() {
                if raw {
                    println!("{}", html);
                } else {
                    println!("{}", strip_ansi(&strip_html_tags(html)));
                }
            }
        });

        Ok(())
    }

    pub fn inbox_delete(&self, args: InboxDeleteArgs) -> Result<()> {
        let count = self.conn.execute("DELETE FROM messages WHERE id = ?1", params![args.id])?;
        if count == 0 {
            bail!("message {} not found", args.id);
        }
        print_success_or(self.format, &serde_json::json!({"id": args.id, "deleted": true}), |_| {
            println!("deleted message {}", args.id);
        });
        Ok(())
    }

    pub fn inbox_archive(&self, args: InboxArchiveArgs) -> Result<()> {
        let count = self.conn.execute(
            "UPDATE messages SET archived = 1 WHERE id = ?1",
            params![args.id],
        )?;
        if count == 0 {
            bail!("message {} not found", args.id);
        }
        print_success_or(self.format, &serde_json::json!({"id": args.id, "archived": true}), |_| {
            println!("archived message {}", args.id);
        });
        Ok(())
    }

    pub fn inbox_search(&self, args: InboxSearchArgs) -> Result<()> {
        let _ = self.conn.execute_batch(
            "INSERT OR REPLACE INTO messages_fts(messages_fts) VALUES('rebuild');"
        );

        let sql = if args.account.is_some() {
            "SELECT m.id, m.remote_id, m.direction, m.account_email, m.from_addr, m.to_json, m.cc_json, m.bcc_json,
                    m.reply_to_json, m.subject, m.text_body, m.html_body, m.rfc_message_id, m.in_reply_to,
                    m.references_json, m.last_event, m.is_read, m.created_at, m.synced_at
             FROM messages m
             JOIN messages_fts fts ON m.id = fts.rowid
             WHERE messages_fts MATCH ?1 AND m.account_email = ?2
             ORDER BY m.created_at DESC
             LIMIT ?3"
        } else {
            "SELECT m.id, m.remote_id, m.direction, m.account_email, m.from_addr, m.to_json, m.cc_json, m.bcc_json,
                    m.reply_to_json, m.subject, m.text_body, m.html_body, m.rfc_message_id, m.in_reply_to,
                    m.references_json, m.last_event, m.is_read, m.created_at, m.synced_at
             FROM messages m
             JOIN messages_fts fts ON m.id = fts.rowid
             WHERE messages_fts MATCH ?1
             ORDER BY m.created_at DESC
             LIMIT ?2"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(account) = &args.account {
            stmt.query_map(
                params![args.query, crate::helpers::normalize_email(account), args.limit as i64],
                crate::db::map_message,
            )?
        } else {
            stmt.query_map(
                params![args.query, args.limit as i64],
                crate::db::map_message,
            )?
        };
        let messages: Vec<_> = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        print_success_or(self.format, &messages, |messages| {
            for message in messages {
                let read_flag = if message.is_read { " " } else { "*" };
                println!(
                    "{}{} [{}] {} | {}",
                    message.id, read_flag, message.direction, message.from_addr, message.subject
                );
            }
            if messages.is_empty() {
                println!("no results");
            }
        });
        Ok(())
    }

    pub fn inbox_purge(&self, args: InboxPurgeArgs) -> Result<()> {
        let count = if let Some(account) = &args.account {
            self.conn.execute(
                "DELETE FROM messages WHERE created_at < ?1 AND account_email = ?2",
                params![args.before, crate::helpers::normalize_email(account)],
            )?
        } else {
            self.conn.execute(
                "DELETE FROM messages WHERE created_at < ?1",
                params![args.before],
            )?
        };
        print_success_or(self.format, &serde_json::json!({"purged": count}), |_| {
            println!("purged {} messages", count);
        });
        Ok(())
    }
}
