# email-cli v0.4 — Agent-First Quality Fixes

Source: GPT Pro review (2026-04-04), reframed for agent-first priority.
Constraint: agents are primary users. GUI is optional upside. Never compromise agent UX.

## Agent-Breaking Bugs (fix now)

### 1. Errors go to stderr in JSON mode
- `output.rs:47-62` prints JSON errors to stderr
- Agents parsing stdout get NOTHING on failure
- Fix: in JSON mode, emit error envelope to stdout. Keep stderr for human diagnostics only.
- Files: `output.rs`, `error.rs`

### 2. FTS rebuilds entire index on every search  
- `inbox.rs:331-333` runs `INSERT OR REPLACE INTO messages_fts(messages_fts) VALUES('rebuild')` on EVERY search
- Gets slower with every message stored
- Fix: add AFTER INSERT/UPDATE/DELETE triggers on `messages` table. Remove rebuild from search path.
- Files: `db.rs` (schema), `inbox.rs` (search method)

### 3. `inbox list` returns full HTML bodies
- `inbox.rs:87-90` selects `text_body`, `html_body` for list
- Agents waste tokens parsing megabytes of body text they didn't ask for
- Fix: list/search/thread summary queries should NOT include text_body/html_body. Only `inbox read` returns bodies.
- Files: `inbox.rs` (list, search, thread methods), `db.rs` (add MessageSummary mapper)

### 4. `archived` missing from map_message and all SELECTs
- Column exists (migration in `app.rs:23-29`) but `db.rs:165-186` map_message doesn't read it
- None of the SELECT queries include `archived`
- Fix: add `archived` to map_message, MessageRecord, and all SELECT column lists
- Files: `db.rs` (map_message, MessageRecord, all queries)

### 5. Error types collapse to `internal_error`
- `error.rs:59-62` maps every `anyhow::Error` to `CliError::Internal`
- Agents can't distinguish "not found" vs "rate limited" vs "bad input"
- Fix: use typed errors at command boundaries. Map not-found, input, config, transient, rate-limit explicitly.
- Files: `error.rs`, `commands/inbox.rs` (bail! calls)

## Agent Quality Improvements (do next)

### 6. Fix cursor to use (created_at, id)
- Current cursor uses `id` only, but `upsert_message` can update `created_at` on existing rows
- Fix: ORDER BY created_at DESC, id DESC. Cursor = (created_at, id) pair.
- Files: `inbox.rs` (list method)

### 7. Add FTS triggers (covered by #2)

### 8. Update agent-info
- Stale: missing `mark`, `thread`, `unarchive`, `--after`, bulk IDs
- Fix: regenerate from current Clap definitions
- Files: `commands/agent_info.rs`

### 9. Thread indexes
- No indexes on `rfc_message_id` or `in_reply_to`
- Fix: add CREATE INDEX in schema
- Files: `db.rs`

### 10. Bulk mutation responses
- Return only count, not which IDs succeeded/failed
- Fix: return `{requested_ids, updated_ids, missing_ids}`
- Files: `inbox.rs` (mark, delete, archive, unarchive methods)
