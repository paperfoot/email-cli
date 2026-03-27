# email-cli Practical Improvements

## Context
email-cli is a 26-file, 4,005-line Rust CLI for AI agents. Seven features move it from "demo" to "production-usable." All must stay lightweight — no async runtime, no heavy deps, fast startup.

## Features

### 1. Terminal output sanitization
- Strip ANSI escape sequences from text bodies in `inbox read`
- Strip HTML tags for human output (simple regex, no dep)
- Add `--raw` flag for unfiltered output
- JSON mode unchanged (serde handles escaping)

### 2. Message delete/archive
- `inbox delete <id>` — CASCADE delete from messages + attachments
- `inbox archive <id>` — set `archived = 1`
- `inbox ls --archived` — show archived messages
- `inbox purge --before <date>` — bulk delete old messages
- Schema: add `archived INTEGER NOT NULL DEFAULT 0` to messages

### 3. Full-text search (FTS5)
- FTS5 virtual table: subject, text_body, html_body, from_addr, to_json, cc_json
- `inbox search <query>` with `--account`, `--limit` filters
- Populated via INSERT trigger + manual rebuild on `sync`
- Zero new deps (FTS5 bundled with rusqlite)

### 4. Draft editing + deletion
- `draft edit <id> [--subject X] [--text X] [--to X]` — UPDATE fields
- `draft delete <id>` — DELETE + cleanup attachment snapshots

### 5. Durable outbox
- New `outbox` table: id, account_email, request_json, idempotency_key, status, attempts, created_at
- Idempotency key: SHA-256 of (from + to_sorted + subject + body)
- `send` writes to outbox first, attempts delivery, marks status
- `outbox list` / `outbox retry <id>` / `outbox flush`
- New dep: `sha2`

### 6. Webhook listener
- `webhook listen --port <port>` — foreground HTTP server
- Accepts POST /webhook with Resend event payload
- New `events` table: id, email_remote_id, event_type, payload_json, created_at
- Updates messages.last_event on delivery events
- `events list [--message <id>]` command
- New dep: `tiny_http`

### 7. Sync optimization
- `sync --watch --interval <secs>` — continuous poll loop
- Per-profile scan with local fan-out to accounts
- Lookback: re-check last 5 messages before cursor for late arrivals

## Schema Changes
```sql
-- messages table: add column
ALTER TABLE messages ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;

-- FTS5
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    subject, text_body, html_body, from_addr, to_json, cc_json,
    content=messages, content_rowid=id
);

-- Outbox
CREATE TABLE IF NOT EXISTS outbox (
    id TEXT PRIMARY KEY,
    account_email TEXT NOT NULL,
    request_json TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Events
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email_remote_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_events_email ON events(email_remote_id);
```

## New Dependencies
- `sha2 = "0.10"` — stable idempotency key hashing
- `tiny_http = "0.12"` — minimal sync HTTP server for webhooks

## Verification
- `cargo check` clean
- `cargo build --release` succeeds
- Test with real Resend API: profile add, send, sync, search, delete, archive, draft edit/delete, outbox retry
