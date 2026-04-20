use anyhow::{Result, bail};
use chrono::{Duration, Utc};
use rusqlite::params;

use crate::app::App;
use crate::cli::{
    InboxArchiveArgs, InboxDeleteArgs, InboxListArgs, InboxMarkArgs, InboxPurgeArgs, InboxReadArgs,
    InboxSearchArgs, InboxSnoozeArgs, InboxStarArgs, InboxStatsArgs, InboxThreadArgs,
    InboxUnarchiveArgs, InboxUnsnoozeArgs, InboxUnsubscribeArgs,
};
use crate::helpers::compact_targets;
use crate::output::print_success_or;

/// Resolve a user-friendly snooze string ("tomorrow", "4h", ISO-8601) into a
/// concrete UTC wake timestamp. Accepts the following forms:
///   - `tonight`           → 7pm local today
///   - `tomorrow`          → 9am local tomorrow
///   - `next-week`         → 9am local next Monday
///   - `<N>h`              → +N hours from now
///   - `<N>d`              → +N days from now
///   - `<N>w`              → +N weeks from now
///   - ISO-8601 timestamp  → as given
fn parse_wake_time(raw: &str) -> Result<chrono::DateTime<Utc>> {
    use chrono::{Datelike, Local, NaiveTime, TimeZone, Weekday};
    let trimmed = raw.trim().to_lowercase();
    let now = Utc::now();

    match trimmed.as_str() {
        "tonight" => {
            let today_local = Local::now().date_naive();
            let wake = today_local
                .and_time(NaiveTime::from_hms_opt(19, 0, 0).unwrap());
            let local = Local
                .from_local_datetime(&wake)
                .single()
                .ok_or_else(|| anyhow::anyhow!("ambiguous local time"))?;
            return Ok(local.with_timezone(&Utc));
        }
        "tomorrow" => {
            let tomorrow_local = Local::now().date_naive().succ_opt().unwrap();
            let wake = tomorrow_local
                .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());
            let local = Local
                .from_local_datetime(&wake)
                .single()
                .ok_or_else(|| anyhow::anyhow!("ambiguous local time"))?;
            return Ok(local.with_timezone(&Utc));
        }
        "next-week" | "nextweek" => {
            let today = Local::now().date_naive();
            let mut d = today.succ_opt().unwrap();
            while d.weekday() != Weekday::Mon {
                d = d.succ_opt().unwrap();
            }
            let wake = d.and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());
            let local = Local
                .from_local_datetime(&wake)
                .single()
                .ok_or_else(|| anyhow::anyhow!("ambiguous local time"))?;
            return Ok(local.with_timezone(&Utc));
        }
        _ => {}
    }

    // Relative like "4h", "2d", "1w".
    if let Some(stripped) = trimmed.strip_suffix('h') {
        if let Ok(n) = stripped.parse::<i64>() {
            return Ok(now + Duration::hours(n));
        }
    }
    if let Some(stripped) = trimmed.strip_suffix('d') {
        if let Ok(n) = stripped.parse::<i64>() {
            return Ok(now + Duration::days(n));
        }
    }
    if let Some(stripped) = trimmed.strip_suffix('w') {
        if let Ok(n) = stripped.parse::<i64>() {
            return Ok(now + Duration::weeks(n));
        }
    }

    // ISO-8601 (with or without timezone).
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(parsed.with_timezone(&Utc));
    }

    anyhow::bail!("couldn't parse snooze time '{}' — try tomorrow, tonight, 4h, 2d, or an ISO timestamp", raw)
}

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
        let now_iso = Utc::now().to_rfc3339();

        let mut conditions = vec!["m.archived = ?".to_string()];
        let mut param_vals: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(archived_val)];

        if let Some(ref account) = args.account {
            conditions.push("m.account_email = ?".to_string());
            param_vals.push(Box::new(crate::helpers::normalize_email(account)));
        }
        if args.unread {
            conditions.push("m.is_read = 0".to_string());
        }
        if args.starred {
            conditions.push("m.starred = 1".to_string());
        }
        if args.snoozed {
            // Only currently-snoozed messages (wake time still in the future).
            conditions.push("m.snoozed_until IS NOT NULL AND m.snoozed_until > ?".to_string());
            param_vals.push(Box::new(now_iso.clone()));
        } else {
            // Default: hide messages whose wake time is still in the future.
            // NULL = never snoozed = always shown.
            conditions.push("(m.snoozed_until IS NULL OR m.snoozed_until <= ?)".to_string());
            param_vals.push(Box::new(now_iso.clone()));
        }
        if let Some(after) = args.after {
            conditions.push("m.id < ?".to_string());
            param_vals.push(Box::new(after));
        }
        param_vals.push(Box::new(fetch_limit));

        let where_clause = conditions.join(" AND ");
        // Summary columns + text_body (for snippet) + new fields. Correlated
        // subquery produces has_attachments in one round-trip.
        let sql = format!(
            "SELECT m.id, m.remote_id, m.direction, m.account_email, m.from_addr, m.to_json, m.cc_json,
                    m.subject, m.rfc_message_id, m.in_reply_to, m.last_event, m.is_read, m.created_at, m.archived,
                    m.text_body, m.starred, m.snoozed_until, m.list_unsubscribe,
                    (SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id) AS has_attachments
             FROM messages m WHERE {} ORDER BY m.created_at DESC, m.id DESC LIMIT ?",
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
        // Summary columns + text_body + new fields so clients can render each
        // thread message with a one-line preview alongside star/snooze chrome.
        let sql = format!(
            "SELECT m.id, m.remote_id, m.direction, m.account_email, m.from_addr, m.to_json, m.cc_json,
                    m.subject, m.rfc_message_id, m.in_reply_to, m.last_event, m.is_read, m.created_at, m.archived,
                    m.text_body, m.starred, m.snoozed_until, m.list_unsubscribe,
                    (SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id) AS has_attachments
             FROM messages m
             WHERE m.rfc_message_id IN ({ph}) OR m.in_reply_to IN ({ph})
             ORDER BY m.created_at ASC"
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
        // Gmail-style operator search. Positional `query` feeds FTS; named
        // flags (--from, --subject, --has-attachment, etc.) become LIKE /
        // equality / EXISTS predicates. At least one constraint must be set.
        let query_trimmed = args.query.trim().to_string();
        let has_any_filter = !query_trimmed.is_empty()
            || args.from.is_some()
            || args.to.is_some()
            || args.subject.is_some()
            || args.has_attachment
            || args.unread
            || args.starred
            || args.account.is_some();
        if !has_any_filter {
            bail!("inbox search needs a query or at least one filter flag (--from, --subject, --has-attachment, --unread, --starred, --account)");
        }

        let mut joins = String::new();
        let mut conditions: Vec<String> = Vec::new();
        let mut param_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if !query_trimmed.is_empty() {
            joins.push_str(" JOIN messages_fts fts ON m.id = fts.rowid");
            conditions.push("messages_fts MATCH ?".to_string());
            param_vals.push(Box::new(query_trimmed.clone()));
        }
        if let Some(ref account) = args.account {
            conditions.push("m.account_email = ?".to_string());
            param_vals.push(Box::new(crate::helpers::normalize_email(account)));
        }
        if let Some(ref from) = args.from {
            conditions.push("m.from_addr LIKE ?".to_string());
            param_vals.push(Box::new(format!("%{}%", from)));
        }
        if let Some(ref to) = args.to {
            conditions.push("m.to_json LIKE ?".to_string());
            param_vals.push(Box::new(format!("%{}%", to)));
        }
        if let Some(ref subj) = args.subject {
            conditions.push("m.subject LIKE ?".to_string());
            param_vals.push(Box::new(format!("%{}%", subj)));
        }
        if args.has_attachment {
            conditions.push(
                "EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id)".to_string(),
            );
        }
        if args.unread {
            conditions.push("m.is_read = 0".to_string());
        }
        if args.starred {
            conditions.push("m.starred = 1".to_string());
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT m.id, m.remote_id, m.direction, m.account_email, m.from_addr, m.to_json, m.cc_json,
                    m.subject, m.rfc_message_id, m.in_reply_to, m.last_event, m.is_read, m.created_at, m.archived,
                    m.text_body, m.starred, m.snoozed_until, m.list_unsubscribe,
                    (SELECT COUNT(*) FROM attachments a2 WHERE a2.message_id = m.id) AS has_attachments
             FROM messages m{joins}
             WHERE {where_clause}
             ORDER BY m.created_at DESC
             LIMIT ?"
        );
        param_vals.push(Box::new(args.limit as i64));

        let refs: Vec<&dyn rusqlite::types::ToSql> = param_vals.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(refs.as_slice(), crate::db::map_summary)?;
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

    pub fn inbox_star(&self, args: InboxStarArgs, starred: bool) -> Result<()> {
        if args.ids.is_empty() {
            bail!("no message IDs provided");
        }
        let placeholders: Vec<String> = (1..=args.ids.len()).map(|i| format!("?{}", i)).collect();
        let ph = placeholders.join(",");
        let new_val: i64 = if starred { 1 } else { 0 };
        let sql = format!(
            "UPDATE messages SET starred = {} WHERE id IN ({})",
            new_val, ph
        );
        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            args.ids.iter().map(|id| Box::new(*id) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let count = self.conn.execute(&sql, refs.as_slice())?;

        let label = if starred { "starred" } else { "unstarred" };
        print_success_or(
            self.format,
            &serde_json::json!({
                "requested_ids": args.ids,
                "count": count,
                "starred": starred,
            }),
            |_| println!("{} {} message(s)", label, count),
        );
        Ok(())
    }

    pub fn inbox_snooze(&self, args: InboxSnoozeArgs) -> Result<()> {
        if args.ids.is_empty() {
            bail!("no message IDs provided");
        }
        let wake = parse_wake_time(&args.until)?;
        let wake_iso = wake.to_rfc3339();

        let placeholders: Vec<String> = (1..=args.ids.len()).map(|i| format!("?{}", i + 1)).collect();
        let ph = placeholders.join(",");
        let sql = format!(
            "UPDATE messages SET snoozed_until = ?1 WHERE id IN ({})",
            ph
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::with_capacity(args.ids.len() + 1);
        params.push(Box::new(wake_iso.clone()));
        for id in &args.ids {
            params.push(Box::new(*id));
        }
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let count = self.conn.execute(&sql, refs.as_slice())?;

        print_success_or(
            self.format,
            &serde_json::json!({
                "requested_ids": args.ids,
                "count": count,
                "snoozed_until": wake_iso,
            }),
            |_| println!("snoozed {} message(s) until {}", count, wake_iso),
        );
        Ok(())
    }

    pub fn inbox_unsnooze(&self, args: InboxUnsnoozeArgs) -> Result<()> {
        if args.ids.is_empty() {
            bail!("no message IDs provided");
        }
        let placeholders: Vec<String> = (1..=args.ids.len()).map(|i| format!("?{}", i)).collect();
        let ph = placeholders.join(",");
        let sql = format!(
            "UPDATE messages SET snoozed_until = NULL WHERE id IN ({})",
            ph
        );
        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            args.ids.iter().map(|id| Box::new(*id) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let count = self.conn.execute(&sql, refs.as_slice())?;

        print_success_or(
            self.format,
            &serde_json::json!({
                "requested_ids": args.ids,
                "count": count,
            }),
            |_| println!("unsnoozed {} message(s)", count),
        );
        Ok(())
    }

    pub fn inbox_unsubscribe(&self, args: InboxUnsubscribeArgs) -> Result<()> {
        let unsub: Option<String> = self.conn.query_row(
            "SELECT list_unsubscribe FROM messages WHERE id = ?1",
            params![args.id],
            |row| row.get(0),
        )?;
        let header = unsub.ok_or_else(|| anyhow::anyhow!("no List-Unsubscribe header on message {}", args.id))?;
        // Extract the first URL or mailto: from the header value. Raw form:
        // `<https://...>, <mailto:...>` — strip angle brackets.
        let url = header
            .split(',')
            .map(|s| s.trim().trim_start_matches('<').trim_end_matches('>').trim())
            .find(|s| s.starts_with("http") || s.starts_with("mailto:"))
            .map(|s| s.to_string())
            .unwrap_or_else(|| header.clone());

        print_success_or(
            self.format,
            &serde_json::json!({
                "id": args.id,
                "url": url,
                "raw_header": header,
            }),
            |_| println!("{}", url),
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
