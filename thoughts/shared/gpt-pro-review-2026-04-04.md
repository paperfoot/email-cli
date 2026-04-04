# GPT Pro Architectural Review — 2026-04-04
## Source: GPT-5.4 Pro, uploaded via structured package

## Scores
| Category | Score | Key Gap |
|---|---:|---|
| JSON API completeness | 4/10 | No API namespace; errors to stderr; inconsistent payloads |
| Daemon quality | 4/10 | Unsafe Send/Sync, global state, mem::forget leaks |
| Database design | 5/10 | Wrong cursor key, FTS rebuild on search, no thread index |
| GUI readiness | 3/10 | No mailbox model, no draft autosave, oversized list payloads |

## Top 5 Fixes (ranked by effort/value)

### 1. Dedicated versioned API namespace (Medium effort, very high value)
- Add `email-cli api ...` commands with frozen response schemas
- Errors to stdout in JSON mode (currently stderr)
- Wire typed errors end-to-end (most errors collapse to `internal_error`)
- Auto-generate agent-info from Clap + response types

### 2. Split MessageSummary from MessageDetail (Medium effort, very high value)
- List/search/thread return summaries: no text_body/html_body
- Add: snippet, has_attachments, attachment_count, archived, thread_count
- Expose mailbox semantics (inbox = received + unarchived)
- Add `inbox stats` for sidebar counts

### 3. Fix query core: cursor, thread, search (Medium effort, high value)
- Cursor: use `(created_at, id)` not just `id`
- Thread: add indexes on rfc_message_id/in_reply_to, or persist thread_key at ingest
- FTS: add triggers instead of rebuild-on-search
- Search: escape user input before FTS MATCH

### 4. Decouple drafts from send (Low-medium effort, high value)
- Allow empty drafts (no required recipients/body)
- Patch-style autosave for GUI
- Separate DraftComposeArgs from ComposeArgs

### 5. Rewrite daemon ownership (Medium effort, medium-high value)
- Replace OnceLock globals with owned DaemonController
- Remove unsafe Send/Sync for UiState
- Remove mem::forget — store in controller struct
- Normalize unread = received AND unarchived AND unread
- Use NSApp.terminate() instead of process::exit(0)

## Critical Bug Findings
- `archived` column missing from `map_message()` and all SELECT queries
- `inbox list` returns sent AND received (not a real inbox)
- FTS rebuild on EVERY search query
- Cursor pagination uses `id` but indexes are on `created_at`
- Bulk mutation responses don't report which IDs succeeded/failed
- Draft creation requires recipients (can't create blank drafts)
