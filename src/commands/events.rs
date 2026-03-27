use anyhow::Result;
use rusqlite::params;

use crate::app::App;
use crate::cli::*;
use crate::output::print_success_or;

impl App {
    pub fn events_list(&self, args: EventsListArgs) -> Result<()> {
        #[derive(serde::Serialize)]
        struct EventRecord {
            id: i64,
            email_remote_id: String,
            event_type: String,
            created_at: String,
        }

        let (sql, messages) = if let Some(message_id) = args.message {
            let message = self.get_message(message_id)?;
            (
                "SELECT id, email_remote_id, event_type, created_at FROM events WHERE email_remote_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                vec![message.remote_id.clone()],
            )
        } else {
            (
                "SELECT id, email_remote_id, event_type, created_at FROM events ORDER BY created_at DESC LIMIT ?2",
                vec![],
            )
        };

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<EventRecord> {
            Ok(EventRecord {
                id: row.get(0)?,
                email_remote_id: row.get(1)?,
                event_type: row.get(2)?,
                created_at: row.get(3)?,
            })
        };

        let mut stmt = self.conn.prepare(sql)?;
        let events: Vec<EventRecord> = if !messages.is_empty() {
            stmt.query_map(params![messages[0], args.limit as i64], map_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![args.limit as i64], map_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        print_success_or(self.format, &events, |events| {
            for event in events {
                println!(
                    "{} {} {}",
                    event.event_type, event.email_remote_id, event.created_at
                );
            }
            if events.is_empty() {
                println!("no events");
            }
        });
        Ok(())
    }

    /// Store an event and update the message's last_event if applicable
    pub fn store_event(&self, remote_id: &str, event_type: &str, payload: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (email_remote_id, event_type, payload_json) VALUES (?1, ?2, ?3)",
            params![remote_id, event_type, payload],
        )?;
        // Update last_event on the message if it exists
        self.conn.execute(
            "UPDATE messages SET last_event = ?1 WHERE remote_id = ?2",
            params![event_type, remote_id],
        )?;
        Ok(())
    }
}
