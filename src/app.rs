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
        // Safe migration: add archived column if missing (idempotent)
        let _ = conn.execute_batch(
            "ALTER TABLE messages ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;"
        );
        Ok(Self {
            conn,
            db_path,
            format,
        })
    }
}
