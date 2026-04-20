use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

use crate::output::Format;

pub struct App {
    pub conn: Connection,
    pub db_path: PathBuf,
    pub format: Format,
}

impl App {
    pub fn new(db_path: PathBuf, format: Format) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open {}", db_path.display()))?;
        conn.execute_batch(crate::db::SCHEMA_DDL)?;
        // Idempotent ALTER TABLE migrations. Each one silently no-ops if the
        // column already exists — SQLite returns "duplicate column" which we
        // swallow via `let _ = ...`.
        let _ = conn
            .execute_batch("ALTER TABLE messages ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;");
        let _ = conn
            .execute_batch("ALTER TABLE messages ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;");
        let _ = conn
            .execute_batch("ALTER TABLE messages ADD COLUMN snoozed_until TEXT;");
        let _ = conn
            .execute_batch("ALTER TABLE messages ADD COLUMN list_unsubscribe TEXT;");
        let _ = conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_messages_archived ON messages(archived, created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_messages_starred ON messages(starred, created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_messages_snoozed ON messages(snoozed_until);"
        );
        // One-off backfill: messages stored before the list_unsubscribe column
        // existed need their headers re-parsed from the cached raw_json so the
        // UI can show unsubscribe buttons on older marketing mail too.
        Self::backfill_list_unsubscribe(&conn);
        Ok(Self {
            conn,
            db_path,
            format,
        })
    }

    /// Walk every received message whose `list_unsubscribe` is still NULL and
    /// try to pluck a value out of the stored raw_json. Idempotent: once a
    /// message has a value, the COALESCE in upsert + our NULL filter here
    /// stop us from ever re-writing it.
    fn backfill_list_unsubscribe(conn: &Connection) {
        use serde_json::Value;
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, raw_json FROM messages
             WHERE list_unsubscribe IS NULL AND direction = 'received'",
        ) else {
            return;
        };
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        });
        let Ok(rows) = rows else { return };

        for result in rows {
            let Ok((id, raw_json)) = result else { continue };
            let Ok(parsed) = serde_json::from_str::<Value>(&raw_json) else { continue };
            let Some(headers) = parsed.get("headers").and_then(|v| v.as_object()) else { continue };

            // Try flat `list-unsubscribe` first.
            let mut value: Option<String> = None;
            for (k, v) in headers {
                if k.eq_ignore_ascii_case("list-unsubscribe") {
                    value = v.as_str().map(|s| s.to_string());
                    break;
                }
            }
            // Fall back to Resend's nested `list.unsubscribe`.
            if value.is_none() {
                let list_val = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("list"))
                    .map(|(_, v)| v);
                if let Some(lv) = list_val {
                    let parsed_list: Value = match lv {
                        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
                        other => other.clone(),
                    };
                    if let Some(unsub) = parsed_list.get("unsubscribe") {
                        let url = unsub.get("url").and_then(|v| v.as_str());
                        let mail = unsub.get("mail").and_then(|v| v.as_str());
                        let mut parts: Vec<String> = Vec::new();
                        if let Some(u) = url { parts.push(format!("<{}>", u)); }
                        if let Some(m) = mail { parts.push(format!("<mailto:{}>", m)); }
                        if !parts.is_empty() { value = Some(parts.join(", ")); }
                    }
                }
            }
            if let Some(v) = value {
                let _ = conn.execute(
                    "UPDATE messages SET list_unsubscribe = ?1 WHERE id = ?2",
                    rusqlite::params![v, id],
                );
            }
        }
    }
}
