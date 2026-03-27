<h1 align="center">email-cli</h1>

<p align="center">
  <strong>Agent-friendly email CLI for Resend. One binary. Local state. 42 commands.</strong><br>
  <em>Built by <a href="https://github.com/199-biotechnologies">Paperfoot AI (SG) Pte. Ltd.</a></em>
</p>

<p align="center">
  <a href="#install">Install</a> &middot;
  <a href="#quick-start">Quick Start</a> &middot;
  <a href="#commands">Commands</a> &middot;
  <a href="#agent-integration">Agent Integration</a>
</p>

---

## What it does

Give an AI agent or terminal operator a Resend-backed email address and let them handle real email without IMAP, a browser inbox, or an MCP server.

- send, receive, reply, draft, sync, attachments
- manage domains, audiences, contacts, API keys
- batch send from JSON files
- local SQLite mailbox with cursor-based sync
- structured JSON output with semantic exit codes
- self-describing capability manifest via `agent-info`
- skill auto-install for Claude Code, Codex CLI, and Gemini CLI

This is not an IMAP server. It is a practical mailbox layer for agents.

## Install

### Homebrew

```bash
brew tap 199-biotechnologies/tap
brew install email-cli
```

### Cargo

```bash
cargo install email-cli
```

### From source

```bash
git clone https://github.com/199-biotechnologies/email-cli.git
cd email-cli
cargo build --release
# binary is at ./target/release/email-cli
```

## Quick start

```bash
# Add a profile (your Resend API key)
email-cli profile add default --api-key-env RESEND_API_KEY

# Check domains
email-cli profile test default

# Add an account
email-cli account add agent@yourdomain.com \
  --profile default \
  --name "Agent" \
  --default

# Send
email-cli send \
  --to someone@example.com \
  --subject "Hello from email-cli" \
  --text "Sent by an agent."

# Sync and read
email-cli sync
email-cli inbox ls
email-cli inbox read 1 --mark-read

# Reply
email-cli reply 1 --text "Got it, thanks."

# Drafts
email-cli draft create \
  --to someone@example.com \
  --subject "Draft" \
  --text "Will send later"
email-cli draft list
email-cli draft send <draft-id>

# Attachments
email-cli attachments list 1
email-cli attachments get 1 <attachment-id> --output ./downloads
```

## Commands

### Core email

| Command | Description |
|---|---|
| `send` | Send an email with text/HTML body and attachments |
| `reply <id>` | Reply to a received message with threading headers |
| `sync` | Sync sent and received messages from Resend into local store |
| `inbox ls` | List messages (filter by account, unread) |
| `inbox read <id>` | Read a message, optionally mark as read |
| `draft create` | Save a local draft with attachment snapshots |
| `draft list` | List drafts |
| `draft show <id>` | Show draft details |
| `draft send <id>` | Send a draft and delete it |
| `attachments list <id>` | List attachments for a message |
| `attachments get <id> <att>` | Download an attachment |

### Account management

| Command | Description |
|---|---|
| `profile add <name>` | Add or update a Resend API profile |
| `profile list` | List profiles |
| `profile test <name>` | Test API connectivity by listing domains |
| `account add <email>` | Register an email identity under a profile |
| `account list` | List accounts |
| `account use <email>` | Set the default sending account |
| `signature set <account>` | Set per-account signature text |
| `signature show <account>` | Show signature |

### Resend management

| Command | Description |
|---|---|
| `domain list` | List domains |
| `domain get <id>` | Get domain details with DNS records |
| `domain create --name <domain>` | Register a new domain |
| `domain verify <id>` | Trigger domain verification |
| `domain delete <id>` | Delete a domain |
| `domain update <id>` | Update tracking settings |
| `audience list` | List audiences |
| `audience get <id>` | Get audience details |
| `audience create --name <name>` | Create an audience |
| `audience delete <id>` | Delete an audience |
| `contact list --audience <id>` | List contacts |
| `contact get --audience <id> <contact_id>` | Get contact details |
| `contact create --audience <id> --email <email>` | Create a contact |
| `contact update --audience <id> <contact_id>` | Update a contact |
| `contact delete --audience <id> <contact_id>` | Delete a contact |
| `batch send --file <path>` | Send batch emails from a JSON file |
| `api-key list` | List API keys |
| `api-key create --name <name>` | Create an API key |
| `api-key delete <id>` | Delete an API key |

### Tooling

| Command | Description |
|---|---|
| `agent-info` | Machine-readable JSON capability manifest |
| `skill install` | Install skill file to Claude/Codex/Gemini |
| `skill status` | Check skill installation |
| `completions <shell>` | Generate shell completions (bash/zsh/fish) |

## Agent integration

`email-cli` follows the [agent-cli-framework](https://github.com/199-biotechnologies/agent-cli-framework) patterns:

### Capability discovery

```bash
email-cli agent-info
```

Returns a JSON manifest of every command, global flags, exit codes, and the output envelope format. An agent calls this once and works from memory. Per-command flags are discoverable via `email-cli <command> --help`.

### Structured output

When piped (or with `--json`), command output is wrapped in a JSON envelope (exceptions: `agent-info` returns raw manifest JSON, `completions` writes shell script directly):

```json
{
  "version": "1",
  "status": "success",
  "data": { ... }
}
```

Errors go to stderr with suggestions:

```json
{
  "version": "1",
  "status": "error",
  "error": {
    "code": "config_error",
    "message": "no default account configured",
    "suggestion": "Run profile add / account add to configure"
  }
}
```

### Semantic exit codes

| Code | Meaning | Agent action |
|---|---|---|
| 0 | Success | Continue |
| 1 | Transient error (network, IO) | Retry |
| 2 | Configuration error | Fix setup |
| 3 | Bad input | Fix arguments |
| 4 | Rate limited | Wait and retry |

### Skill self-install

```bash
email-cli skill install
```

Writes `SKILL.md` to `~/.claude/skills/email-cli/`, `~/.codex/skills/email-cli/`, and `~/.gemini/skills/email-cli/`. The skill is a signpost that tells agents the CLI exists and to run `agent-info` for the rest.

## Architecture

Three core concepts:

1. **Profile** -- a Resend API context (API key)
2. **Account** -- a logical sender/receiver identity (e.g. `agent@yourdomain.com`)
3. **Local mailbox** -- SQLite database with messages, drafts, sync cursors, attachments

Resend handles delivery. `email-cli` handles the local operating model agents need.

### Local state

Metadata lives in `~/.local/share/email-cli/email-cli.db` (or `--db <path>`). Sibling directories hold file state: `draft-attachments/` for draft snapshots, `downloads/` for fetched attachments (configurable via `--output`). Tables:

- `profiles` -- API key storage
- `accounts` -- email identities with signatures
- `messages` -- sent and received messages with full metadata
- `drafts` -- local drafts with attachment snapshots
- `attachments` -- attachment metadata and local paths
- `sync_state` -- per-account cursor positions

SQLite is configured with WAL mode, busy timeout, and foreign keys.

## Security

- API keys are stored in the local SQLite database -- treat it as sensitive
- Do not commit API keys or `.env` files
- Attachment filenames are sanitized before local writes
- Each send includes an `Idempotency-Key` header (UUID per call; not stable across retries)
- Use `--api-key-env VAR_NAME` or `--api-key-file path/to/.env` (expects `NAME=value` format) instead of `--api-key` directly

## Requirements

- A Resend API key with sending enabled
- A verified Resend domain (receiving enabled on the domain if you want inbox sync)
- Rust 1.85+ toolchain (if building from source -- required for edition 2024)

## License

MIT -- see [LICENSE](LICENSE).
