use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use crate::app::App;
use crate::helpers::{
    effective_received_bcc, effective_received_cc, effective_received_to, from_json,
    header_references, header_string, normalize_email, normalize_timestamp, to_json,
};
use crate::models::*;

// ── Schema DDL ───────────────────────────────────────────────────────────────

pub const SCHEMA_DDL: &str = "
    PRAGMA foreign_keys = ON;
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = NORMAL;
    PRAGMA busy_timeout = 5000;

    CREATE TABLE IF NOT EXISTS profiles (
        name TEXT PRIMARY KEY,
        api_key TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS accounts (
        email TEXT PRIMARY KEY,
        profile_name TEXT NOT NULL REFERENCES profiles(name),
        display_name TEXT,
        signature TEXT NOT NULL DEFAULT '',
        is_default INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_default
    ON accounts(is_default) WHERE is_default = 1;

    CREATE TABLE IF NOT EXISTS messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        remote_id TEXT NOT NULL,
        direction TEXT NOT NULL,
        account_email TEXT NOT NULL REFERENCES accounts(email),
        from_addr TEXT NOT NULL,
        to_json TEXT NOT NULL,
        cc_json TEXT NOT NULL DEFAULT '[]',
        bcc_json TEXT NOT NULL DEFAULT '[]',
        reply_to_json TEXT NOT NULL DEFAULT '[]',
        subject TEXT NOT NULL DEFAULT '',
        text_body TEXT,
        html_body TEXT,
        rfc_message_id TEXT,
        in_reply_to TEXT,
        references_json TEXT NOT NULL DEFAULT '[]',
        last_event TEXT,
        is_read INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        synced_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        raw_json TEXT NOT NULL,
        UNIQUE(remote_id, direction, account_email)
    );

    CREATE TABLE IF NOT EXISTS drafts (
        id TEXT PRIMARY KEY,
        account_email TEXT NOT NULL REFERENCES accounts(email),
        to_json TEXT NOT NULL,
        cc_json TEXT NOT NULL DEFAULT '[]',
        bcc_json TEXT NOT NULL DEFAULT '[]',
        subject TEXT NOT NULL DEFAULT '',
        text_body TEXT,
        html_body TEXT,
        reply_to_message_id INTEGER REFERENCES messages(id),
        attachment_paths_json TEXT NOT NULL DEFAULT '[]',
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS attachments (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
        remote_attachment_id TEXT,
        filename TEXT,
        content_type TEXT,
        size INTEGER,
        download_url TEXT,
        local_path TEXT,
        raw_json TEXT NOT NULL,
        UNIQUE(message_id, remote_attachment_id, filename)
    );

    CREATE TABLE IF NOT EXISTS sync_state (
        account_email TEXT NOT NULL REFERENCES accounts(email) ON DELETE CASCADE,
        direction TEXT NOT NULL,
        cursor_id TEXT,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        PRIMARY KEY (account_email, direction)
    );

    CREATE INDEX IF NOT EXISTS idx_messages_account_created
    ON messages(account_email, created_at DESC);

    CREATE INDEX IF NOT EXISTS idx_messages_account_unread_created
    ON messages(account_email, is_read, created_at DESC);

    CREATE INDEX IF NOT EXISTS idx_drafts_account_updated
    ON drafts(account_email, updated_at DESC);

    CREATE INDEX IF NOT EXISTS idx_attachments_message
    ON attachments(message_id);
";

// ── Row mappers ──────────────────────────────────────────────────────────────

pub fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<AccountRecord> {
    Ok(AccountRecord {
        email: row.get(0)?,
        profile_name: row.get(1)?,
        display_name: row.get(2)?,
        signature: row.get(3)?,
        is_default: row.get::<_, i64>(4)? == 1,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        id: row.get(0)?,
        remote_id: row.get(1)?,
        direction: row.get(2)?,
        account_email: row.get(3)?,
        from_addr: row.get(4)?,
        to: from_json(&row.get::<_, String>(5)?).unwrap_or_default(),
        cc: from_json(&row.get::<_, String>(6)?).unwrap_or_default(),
        bcc: from_json(&row.get::<_, String>(7)?).unwrap_or_default(),
        reply_to: from_json(&row.get::<_, String>(8)?).unwrap_or_default(),
        subject: row.get(9)?,
        text_body: row.get(10)?,
        html_body: row.get(11)?,
        rfc_message_id: row.get(12)?,
        in_reply_to: row.get(13)?,
        references: from_json(&row.get::<_, String>(14)?).unwrap_or_default(),
        last_event: row.get(15)?,
        is_read: row.get::<_, i64>(16)? == 1,
        created_at: row.get(17)?,
        synced_at: row.get(18)?,
    })
}

pub fn map_draft(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftRecord> {
    Ok(DraftRecord {
        id: row.get(0)?,
        account_email: row.get(1)?,
        to: from_json(&row.get::<_, String>(2)?).unwrap_or_default(),
        cc: from_json(&row.get::<_, String>(3)?).unwrap_or_default(),
        bcc: from_json(&row.get::<_, String>(4)?).unwrap_or_default(),
        subject: row.get(5)?,
        text_body: row.get(6)?,
        html_body: row.get(7)?,
        reply_to_message_id: row.get(8)?,
        attachment_paths: from_json(&row.get::<_, String>(9)?).unwrap_or_default(),
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub fn map_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRecord> {
    Ok(AttachmentRecord {
        id: row.get(0)?,
        message_id: row.get(1)?,
        remote_attachment_id: row.get(2)?,
        filename: row.get(3)?,
        content_type: row.get(4)?,
        size: row.get(5)?,
        download_url: row.get(6)?,
        local_path: row.get(7)?,
    })
}

// ── Database methods ─────────────────────────────────────────────────────────

impl App {
    pub fn client_for_profile(&self, name: &str) -> Result<crate::resend::ResendClient> {
        let api_key: String = self
            .conn
            .query_row(
                "SELECT api_key FROM profiles WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .with_context(|| format!("profile {} not found", name))?;
        crate::resend::ResendClient::new(api_key)
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT email, profile_name, display_name, signature, is_default, created_at, updated_at
            FROM accounts
            ORDER BY is_default DESC, email
            ",
        )?;
        let rows = stmt.query_map([], map_account)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_account(&self, email: &str) -> Result<AccountRecord> {
        self.conn
            .query_row(
                "
                SELECT email, profile_name, display_name, signature, is_default, created_at, updated_at
                FROM accounts
                WHERE email = ?1
                ",
                params![email],
                map_account,
            )
            .with_context(|| format!("account {} not found", email))
    }

    pub fn default_account(&self) -> Result<AccountRecord> {
        self.conn
            .query_row(
                "
                SELECT email, profile_name, display_name, signature, is_default, created_at, updated_at
                FROM accounts
                WHERE is_default = 1
                LIMIT 1
                ",
                [],
                map_account,
            )
            .context("no default account configured")
    }

    pub fn get_message(&self, id: i64) -> Result<MessageRecord> {
        self.conn
            .query_row(
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE id = ?1
                ",
                params![id],
                map_message,
            )
            .with_context(|| format!("message {} not found", id))
    }

    pub fn list_messages(
        &self,
        account: Option<&str>,
        limit: usize,
        unread_only: bool,
    ) -> Result<Vec<MessageRecord>> {
        let sql = match (account, unread_only) {
            (Some(_), true) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE account_email = ?1 AND is_read = 0
                ORDER BY created_at DESC
                LIMIT ?2
                "
            }
            (Some(_), false) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE account_email = ?1
                ORDER BY created_at DESC
                LIMIT ?2
                "
            }
            (None, true) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE is_read = 0
                ORDER BY created_at DESC
                LIMIT ?1
                "
            }
            (None, false) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                ORDER BY created_at DESC
                LIMIT ?1
                "
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match (account, unread_only) {
            (Some(account), _) => {
                stmt.query_map(params![normalize_email(account), limit as i64], map_message)?
            }
            (None, _) => stmt.query_map(params![limit as i64], map_message)?,
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_all_drafts(&self) -> Result<Vec<DraftRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, account_email, to_json, cc_json, bcc_json, subject, text_body, html_body,
                   reply_to_message_id, attachment_paths_json, created_at, updated_at
            FROM drafts
            ORDER BY updated_at DESC
            ",
        )?;
        let rows = stmt.query_map([], map_draft)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_drafts_for_account(&self, account: &str) -> Result<Vec<DraftRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, account_email, to_json, cc_json, bcc_json, subject, text_body, html_body,
                   reply_to_message_id, attachment_paths_json, created_at, updated_at
            FROM drafts
            WHERE account_email = ?1
            ORDER BY updated_at DESC
            ",
        )?;
        let rows = stmt.query_map(params![account], map_draft)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_draft(&self, id: &str) -> Result<DraftRecord> {
        self.conn
            .query_row(
                "
                SELECT id, account_email, to_json, cc_json, bcc_json, subject, text_body, html_body,
                       reply_to_message_id, attachment_paths_json, created_at, updated_at
                FROM drafts
                WHERE id = ?1
                ",
                params![id],
                map_draft,
            )
            .with_context(|| format!("draft {} not found", id))
    }

    pub fn list_attachments(&self, message_id: i64) -> Result<Vec<AttachmentRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, message_id, remote_attachment_id, filename, content_type, size, download_url, local_path
            FROM attachments
            WHERE message_id = ?1
            ORDER BY id
            ",
        )?;
        let rows = stmt.query_map(params![message_id], map_attachment)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn find_attachment(
        &self,
        message_id: i64,
        attachment_id: &str,
    ) -> Result<Option<AttachmentRecord>> {
        self.conn
            .query_row(
                "
                SELECT id, message_id, remote_attachment_id, filename, content_type, size, download_url, local_path
                FROM attachments
                WHERE message_id = ?1 AND remote_attachment_id = ?2
                ",
                params![message_id, attachment_id],
                map_attachment,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_sync_cursor(
        &self,
        account_email: &str,
        direction: &str,
    ) -> Result<Option<String>> {
        self.conn
            .query_row(
                "
                SELECT cursor_id
                FROM sync_state
                WHERE account_email = ?1 AND direction = ?2
                ",
                params![account_email, direction],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn set_sync_cursor(
        &self,
        account_email: &str,
        direction: &str,
        cursor_id: &str,
    ) -> Result<()> {
        self.conn.execute(
            "
            INSERT INTO sync_state (account_email, direction, cursor_id, updated_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT(account_email, direction) DO UPDATE SET
                cursor_id = excluded.cursor_id,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![account_email, direction, cursor_id],
        )?;
        Ok(())
    }

    pub fn store_sent_message(
        &self,
        account: &AccountRecord,
        email: SentEmail,
        reply_headers: Option<ReplyHeaders>,
    ) -> Result<MessageRecord> {
        let raw_json = serde_json::to_string(&email)?;
        let created_at = normalize_timestamp(email.created_at.as_deref());
        let references = reply_headers
            .as_ref()
            .map(|reply| reply.references.clone())
            .unwrap_or_default();
        let in_reply_to = reply_headers.and_then(|reply| reply.in_reply_to);
        self.upsert_message(MessageUpsert {
            remote_id: email.id,
            direction: "sent".to_string(),
            account_email: account.email.clone(),
            from_addr: email.from.unwrap_or_else(|| account.email.clone()),
            to: email.to,
            cc: email.cc,
            bcc: email.bcc,
            reply_to: email.reply_to,
            subject: email.subject.unwrap_or_default(),
            text_body: email.text,
            html_body: email.html,
            rfc_message_id: None,
            in_reply_to,
            references,
            last_event: email.last_event,
            is_read: true,
            created_at,
            raw_json,
        })
    }

    pub fn store_received_message(
        &self,
        account: &AccountRecord,
        email: ReceivedEmail,
    ) -> Result<i64> {
        let raw_json = serde_json::to_string(&email)?;
        let created_at = normalize_timestamp(email.created_at.as_deref());
        let headers = email.headers.clone().unwrap_or_default();
        let references = header_references(&headers);
        let in_reply_to = header_string(&headers, "in-reply-to");
        let to = effective_received_to(&email);
        let cc = effective_received_cc(&email);
        let bcc = effective_received_bcc(&email);
        let record = self.upsert_message(MessageUpsert {
            remote_id: email.id.clone(),
            direction: "received".to_string(),
            account_email: account.email.clone(),
            from_addr: email.from.unwrap_or_default(),
            to,
            cc,
            bcc,
            reply_to: email.reply_to.clone(),
            subject: email.subject.unwrap_or_default(),
            text_body: email.text.clone(),
            html_body: email.html.clone(),
            rfc_message_id: email.message_id.clone(),
            in_reply_to,
            references,
            last_event: Some("received".to_string()),
            is_read: false,
            created_at,
            raw_json,
        })?;
        Ok(record.id)
    }

    pub fn store_received_attachments(
        &self,
        message_id: i64,
        attachments: &[ReceivedAttachment],
    ) -> Result<()> {
        for attachment in attachments {
            self.conn.execute(
                "
                INSERT INTO attachments (
                    message_id, remote_attachment_id, filename, content_type, size, download_url, raw_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(message_id, remote_attachment_id, filename) DO UPDATE SET
                    content_type = excluded.content_type,
                    size = excluded.size,
                    download_url = excluded.download_url,
                    raw_json = excluded.raw_json
                ",
                params![
                    message_id,
                    attachment.id,
                    attachment.filename,
                    attachment.content_type,
                    attachment.size,
                    attachment.download_url,
                    serde_json::to_string(attachment)?,
                ],
            )?;
        }
        Ok(())
    }

    pub fn upsert_message(&self, payload: MessageUpsert) -> Result<MessageRecord> {
        let existing_id: Option<i64> = self
            .conn
            .query_row(
                "
                SELECT id
                FROM messages
                WHERE remote_id = ?1 AND direction = ?2 AND account_email = ?3
                ",
                params![payload.remote_id, payload.direction, payload.account_email],
                |row| row.get(0),
            )
            .optional()?;

        match existing_id {
            Some(id) => {
                self.conn.execute(
                    "
                    UPDATE messages
                    SET from_addr = ?1,
                        to_json = ?2,
                        cc_json = ?3,
                        bcc_json = ?4,
                        reply_to_json = ?5,
                        subject = ?6,
                        text_body = ?7,
                        html_body = ?8,
                        rfc_message_id = COALESCE(?9, rfc_message_id),
                        in_reply_to = COALESCE(?10, in_reply_to),
                        references_json = ?11,
                        last_event = ?12,
                        created_at = ?13,
                        synced_at = CURRENT_TIMESTAMP,
                        raw_json = ?14
                    WHERE id = ?15
                    ",
                    params![
                        payload.from_addr,
                        to_json(&payload.to)?,
                        to_json(&payload.cc)?,
                        to_json(&payload.bcc)?,
                        to_json(&payload.reply_to)?,
                        payload.subject,
                        payload.text_body,
                        payload.html_body,
                        payload.rfc_message_id,
                        payload.in_reply_to,
                        to_json(&payload.references)?,
                        payload.last_event,
                        payload.created_at,
                        payload.raw_json,
                        id,
                    ],
                )?;
                self.get_message(id)
            }
            None => {
                self.conn.execute(
                    "
                    INSERT INTO messages (
                        remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                        reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                        references_json, last_event, is_read, created_at, raw_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                    ",
                    params![
                        payload.remote_id,
                        payload.direction,
                        payload.account_email,
                        payload.from_addr,
                        to_json(&payload.to)?,
                        to_json(&payload.cc)?,
                        to_json(&payload.bcc)?,
                        to_json(&payload.reply_to)?,
                        payload.subject,
                        payload.text_body,
                        payload.html_body,
                        payload.rfc_message_id,
                        payload.in_reply_to,
                        to_json(&payload.references)?,
                        payload.last_event,
                        if payload.is_read { 1 } else { 0 },
                        payload.created_at,
                        payload.raw_json,
                    ],
                )?;
                let id = self.conn.last_insert_rowid();
                self.get_message(id)
            }
        }
    }
}
