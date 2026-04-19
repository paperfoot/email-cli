use anyhow::{Result, bail};
use rusqlite::params;

use crate::app::App;
use crate::cli::{
    InboxArchiveArgs, InboxDeleteArgs, InboxListArgs, InboxMarkArgs, InboxPurgeArgs, InboxReadArgs,
    InboxSearchArgs, InboxStatsArgs, InboxThreadArgs, InboxUnarchiveArgs,
};
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
        let fetch_limit = (args.limit + 1) as i64;

        let mut conditions = vec!["archived = ?"];
        let mut param_vals: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(archived_val)];

        if let Some(ref account) = args.account {
            conditions.push("account_email = ?");
            param_vals.push(Box::new(crate::helpers::normalize_email(account)));
        }
        if args.unread {
            conditions.push("is_read = 0");
        }
        if let Some(after) = args.after {
            conditions.push("id < ?");
            param_vals.push(Box::new(after));
        }
        param_vals.push(Box::new(fetch_limit));

        let where_clause = conditions.join(" AND ");
        // Summary columns + text_body for a one-line preview snippet. The mapper
        // distils the body down to ~160 chars; no html_body to keep payload small.
        let sql = format!(
            "SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json,
                    subject, rfc_message_id, in_reply_to, last_event, is_read, created_at, archived,
                    text_body
             FROM messages WHERE {} ORDER BY created_at DESC, id DESC LIMIT ?",
            where_clause
        );

        let refs: Vec<&dyn rusqlite::types::ToSql> =
            param_vals.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(refs.as_slice(), crate::db::map_summary)?;
        let mut messages: Vec<_> = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        let has_more = messages.len() > args.limit;
        if has_more {
            messages.truncate(args.limit);
        }
        let next_cursor = messages.last().map(|m| m.id);

        let response = serde_json::json!({
            "messages": messages,
            "has_more": has_more,
            "next_cursor": next_cursor,
        });

        print_success_or(self.format, &response, |_| {
            for message in &messages {
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
            if has_more && let Some(cursor) = next_cursor {
                println!("--- more results: --after {}", cursor);
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

    pub fn inbox_mark(&self, args: InboxMarkArgs) -> Result<()> {
        if args.ids.is_empty() {
            bail!("no message IDs provided");
        }
        let new_state: i64 = if args.unread { 0 } else { 1 };
        let placeholders: Vec<String> = (1..=args.ids.len()).map(|i| format!("?{}", i)).collect();
        let ph = placeholders.join(",");
        let sql = format!(
            "UPDATE messages SET is_read = {} WHERE id IN ({})",
            new_state, ph
        );
        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            args.ids.iter().map(|id| Box::new(*id) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, refs.as_slice())?;

        // Query back which requested IDs actually exist in the table
        let select_sql = format!("SELECT id FROM messages WHERE id IN ({})", ph);
        let mut stmt = self.conn.prepare(&select_sql)?;
        let existing: Vec<i64> = stmt
            .query_map(refs.as_slice(), |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();
        let updated_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| existing.contains(id))
            .collect();
        let missing_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| !existing.contains(id))
            .collect();
        let count = updated_ids.len();

        let label = if args.unread { "unread" } else { "read" };
        print_success_or(
            self.format,
            &serde_json::json!({
                "requested_ids": args.ids,
                "updated_ids": updated_ids,
                "missing_ids": missing_ids,
                "count": count,
                "is_read": new_state == 1,
            }),
            |_| println!("marked {} message(s) as {}", count, label),
        );
        Ok(())
    }

    pub fn inbox_delete(&self, args: InboxDeleteArgs) -> Result<()> {
        if args.ids.is_empty() {
            bail!("no message IDs provided");
        }
        let placeholders: Vec<String> = (1..=args.ids.len()).map(|i| format!("?{}", i)).collect();
        let ph = placeholders.join(",");
        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            args.ids.iter().map(|id| Box::new(*id) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        // Before deleting, find which requested IDs actually exist
        let select_sql = format!("SELECT id FROM messages WHERE id IN ({})", ph);
        let mut stmt = self.conn.prepare(&select_sql)?;
        let existing: Vec<i64> = stmt
            .query_map(refs.as_slice(), |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();
        let deleted_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| existing.contains(id))
            .collect();
        let missing_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| !existing.contains(id))
            .collect();

        let sql = format!("DELETE FROM messages WHERE id IN ({})", ph);
        self.conn.execute(&sql, refs.as_slice())?;
        let count = deleted_ids.len();

        if count == 0 {
            bail!("no matching messages found");
        }
        print_success_or(
            self.format,
            &serde_json::json!({
                "requested_ids": args.ids,
                "deleted_ids": deleted_ids,
                "missing_ids": missing_ids,
                "count": count,
            }),
            |_| println!("deleted {} message(s)", count),
        );
        Ok(())
    }

    pub fn inbox_archive(&self, args: InboxArchiveArgs) -> Result<()> {
        if args.ids.is_empty() {
            bail!("no message IDs provided");
        }
        let placeholders: Vec<String> = (1..=args.ids.len()).map(|i| format!("?{}", i)).collect();
        let ph = placeholders.join(",");
        let sql = format!("UPDATE messages SET archived = 1 WHERE id IN ({})", ph);
        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            args.ids.iter().map(|id| Box::new(*id) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, refs.as_slice())?;

        // Query back which requested IDs actually exist in the table
        let select_sql = format!("SELECT id FROM messages WHERE id IN ({})", ph);
        let mut stmt = self.conn.prepare(&select_sql)?;
        let existing: Vec<i64> = stmt
            .query_map(refs.as_slice(), |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();
        let updated_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| existing.contains(id))
            .collect();
        let missing_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| !existing.contains(id))
            .collect();
        let count = updated_ids.len();

        if count == 0 {
            bail!("no matching messages found");
        }
        print_success_or(
            self.format,
            &serde_json::json!({
                "requested_ids": args.ids,
                "updated_ids": updated_ids,
                "missing_ids": missing_ids,
                "count": count,
            }),
            |_| println!("archived {} message(s)", count),
        );
        Ok(())
    }

    pub fn inbox_unarchive(&self, args: InboxUnarchiveArgs) -> Result<()> {
        if args.ids.is_empty() {
            bail!("no message IDs provided");
        }
        let placeholders: Vec<String> = (1..=args.ids.len()).map(|i| format!("?{}", i)).collect();
        let ph = placeholders.join(",");
        let sql = format!("UPDATE messages SET archived = 0 WHERE id IN ({})", ph);
        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            args.ids.iter().map(|id| Box::new(*id) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, refs.as_slice())?;

        // Query back which requested IDs actually exist in the table
        let select_sql = format!("SELECT id FROM messages WHERE id IN ({})", ph);
        let mut stmt = self.conn.prepare(&select_sql)?;
        let existing: Vec<i64> = stmt
            .query_map(refs.as_slice(), |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();
        let updated_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| existing.contains(id))
            .collect();
        let missing_ids: Vec<i64> = args
            .ids
            .iter()
            .copied()
            .filter(|id| !existing.contains(id))
            .collect();
        let count = updated_ids.len();

        if count == 0 {
            bail!("no matching messages found");
        }
        print_success_or(
            self.format,
            &serde_json::json!({
                "requested_ids": args.ids,
                "updated_ids": updated_ids,
                "missing_ids": missing_ids,
                "count": count,
            }),
            |_| println!("unarchived {} message(s)", count),
        );
        Ok(())
    }

    pub fn inbox_thread(&self, args: InboxThreadArgs) -> Result<()> {
        // 1. Get the seed message
        let seed = self.get_message(args.id)?;

        // 2. Collect all known message-ids in the thread
        let mut thread_ids: Vec<String> = Vec::new();
        if let Some(ref mid) = seed.rfc_message_id {
            thread_ids.push(mid.clone());
        }
        if let Some(ref irt) = seed.in_reply_to {
            thread_ids.push(irt.clone());
        }
        for r in &seed.references {
            if !thread_ids.contains(r) {
                thread_ids.push(r.clone());
            }
        }

        if thread_ids.is_empty() {
            // No threading info — return just the seed message
            print_success_or(self.format, &vec![&seed], |msgs| {
                for m in msgs {
                    println!("{} [{}] {} | {}", m.id, m.direction, m.from_addr, m.subject);
                }
            });
            return Ok(());
        }

        // 3. Find all messages whose rfc_message_id OR in_reply_to is in thread_ids
        let placeholders: Vec<String> = (1..=thread_ids.len()).map(|i| format!("?{}", i)).collect();
        let ph = placeholders.join(",");
        // Summary columns + text_body so callers can render each thread
        // message with a one-line preview alongside the sender/subject.
        let sql = format!(
            "SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json,
                    subject, rfc_message_id, in_reply_to, last_event, is_read, created_at, archived,
                    text_body
             FROM messages
             WHERE rfc_message_id IN ({ph}) OR in_reply_to IN ({ph})
             ORDER BY created_at ASC"
        );

        let mut param_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for id in &thread_ids {
            param_vals.push(Box::new(id.clone()));
        }
        let refs: Vec<&dyn rusqlite::types::ToSql> =
            param_vals.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(refs.as_slice(), crate::db::map_summary)?;
        let messages: Vec<_> = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        print_success_or(self.format, &messages, |messages| {
            for m in messages {
                let read_flag = if m.is_read { " " } else { "*" };
                println!(
                    "{}{} [{}] {} | {}",
                    m.id, read_flag, m.direction, m.from_addr, m.subject
                );
            }
            println!("--- {} messages in thread", messages.len());
        });
        Ok(())
    }

    pub fn inbox_search(&self, args: InboxSearchArgs) -> Result<()> {
        // Summary columns + text_body so search results show a preview snippet.
        let sql = if args.account.is_some() {
            "SELECT m.id, m.remote_id, m.direction, m.account_email, m.from_addr, m.to_json, m.cc_json,
                    m.subject, m.rfc_message_id, m.in_reply_to, m.last_event, m.is_read, m.created_at, m.archived,
                    m.text_body
             FROM messages m
             JOIN messages_fts fts ON m.id = fts.rowid
             WHERE messages_fts MATCH ?1 AND m.account_email = ?2
             ORDER BY m.created_at DESC
             LIMIT ?3"
        } else {
            "SELECT m.id, m.remote_id, m.direction, m.account_email, m.from_addr, m.to_json, m.cc_json,
                    m.subject, m.rfc_message_id, m.in_reply_to, m.last_event, m.is_read, m.created_at, m.archived,
                    m.text_body
             FROM messages m
             JOIN messages_fts fts ON m.id = fts.rowid
             WHERE messages_fts MATCH ?1
             ORDER BY m.created_at DESC
             LIMIT ?2"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(account) = &args.account {
            stmt.query_map(
                params![
                    args.query,
                    crate::helpers::normalize_email(account),
                    args.limit as i64
                ],
                crate::db::map_summary,
            )?
        } else {
            stmt.query_map(
                params![args.query, args.limit as i64],
                crate::db::map_summary,
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

    pub fn inbox_stats(&self, args: InboxStatsArgs) -> Result<()> {
        let (total, unread, archived, sent) = if let Some(ref account) = args.account {
            let acct = crate::helpers::normalize_email(account);
            let total: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE account_email = ?1",
                params![acct],
                |r| r.get(0),
            )?;
            let unread: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE account_email = ?1 AND is_read = 0 AND direction = 'received' AND archived = 0",
                params![acct], |r| r.get(0),
            )?;
            let archived: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE account_email = ?1 AND archived = 1",
                params![acct],
                |r| r.get(0),
            )?;
            let sent: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE account_email = ?1 AND direction = 'sent'",
                params![acct],
                |r| r.get(0),
            )?;
            (total, unread, archived, sent)
        } else {
            let total: i64 = self
                .conn
                .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
            let unread: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE is_read = 0 AND direction = 'received' AND archived = 0",
                [], |r| r.get(0),
            )?;
            let archived: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE archived = 1",
                [],
                |r| r.get(0),
            )?;
            let sent: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE direction = 'sent'",
                [],
                |r| r.get(0),
            )?;
            (total, unread, archived, sent)
        };
        let inbox = total - archived - sent;
        print_success_or(
            self.format,
            &serde_json::json!({
                "total": total,
                "inbox": inbox,
                "unread": unread,
                "archived": archived,
                "sent": sent,
            }),
            |_| {
                println!("total: {}", total);
                println!("inbox: {} ({} unread)", inbox, unread);
                println!("archived: {}", archived);
                println!("sent: {}", sent);
            },
        );
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
