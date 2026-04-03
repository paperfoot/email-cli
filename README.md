<div align="center">

# Email CLI

**Send, receive, and manage email from your terminal. Built for AI agents.**

<br />

[![Star this repo](https://img.shields.io/github/stars/199-biotechnologies/email-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/199-biotechnologies/email-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

<br />

[![Crates.io](https://img.shields.io/crates/v/email-cli?style=for-the-badge&logo=rust&logoColor=white&label=crates.io)](https://crates.io/crates/email-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Homebrew](https://img.shields.io/badge/Homebrew-tap-FBB040?style=for-the-badge&logo=homebrew&logoColor=white)](https://github.com/199-biotechnologies/homebrew-tap)

---

A single binary that gives your AI agent a real email address. Send, receive, reply, draft, sync, and manage contacts through Resend -- all from the command line. No IMAP. No browser inbox. No MCP server. Just a local SQLite mailbox and 50+ commands.

[Why This Exists](#why-this-exists) | [Install](#install) | [How It Works](#how-it-works) | [Commands](#commands) | [Configuration](#configuration) | [Contributing](#contributing)

</div>

## Why This Exists

AI agents need to send and receive email. The existing options are bad:

- **IMAP/SMTP** requires complex server configuration, credential management, and connection handling. Agents struggle with it.
- **Email APIs** work for sending, but agents need a local mailbox to track threads, drafts, and read status.
- **Browser-based inboxes** are not scriptable. Agents cannot use them.

Email CLI solves this by wrapping the [Resend API](https://resend.com) in a local-first CLI. Your agent gets a verified email address, a local SQLite mailbox, and structured JSON output with semantic exit codes. It calls `agent-info` once to learn every command, then works from memory.

You can also use it as a human. It works the same way.

## Install

### One-line install (pre-built binary, no Rust required)

```bash
curl -fsSL https://raw.githubusercontent.com/199-biotechnologies/email-cli/main/install.sh | sh
```

### Homebrew

```bash
brew tap 199-biotechnologies/tap
brew install email-cli
```

### Cargo

```bash
cargo install email-cli
```

### Update

```bash
email-cli update           # self-update from GitHub Releases
email-cli update --check   # check without installing
```

## Quick Start

```bash
# 1. Add your Resend API key
email-cli profile add default --api-key-env RESEND_API_KEY

# 2. Verify domain connectivity
email-cli profile test default

# 3. Register a sending identity
email-cli account add agent@yourdomain.com \
  --profile default \
  --name "Agent" \
  --default

# 4. Send an email
email-cli send \
  --to someone@example.com \
  --subject "Hello from email-cli" \
  --text "Sent from the terminal."

# 5. Sync and read incoming mail
email-cli sync
email-cli inbox ls
email-cli inbox read 1 --mark-read

# 6. Reply (threads correctly with In-Reply-To + References)
email-cli reply 1 --text "Got it, thanks."

# 7. Reply All (preserves CC recipients)
email-cli reply 1 --all --text "Thanks everyone."

# 8. Forward
email-cli forward 1 --to colleague@example.com --text "FYI"

# 9. Thread a new send into an existing conversation
email-cli send --reply-to-msg 1 --to someone@example.com --text "Following up"
```

## How It Works

Three concepts:

1. **Profile** -- a Resend API key. You can have multiple profiles for different Resend accounts.
2. **Account** -- a sender/receiver identity (`agent@yourdomain.com`). Each account belongs to a profile.
3. **Local mailbox** -- a SQLite database that stores messages, drafts, attachments, and sync cursors.

Resend handles delivery. Email CLI handles the local operating model that agents need: read tracking, threading, drafts, batch sends, and structured output.

```
┌────────────────────────────────┐
│         Your Agent / You       │
│    (Claude, Codex, Gemini)     │
└──────────────┬─────────────────┘
               │  CLI commands
               ▼
┌────────────────────────────────┐
│          email-cli             │
│   50+ commands, JSON output,   │
│   semantic exit codes          │
└──────────┬─────────┬───────────┘
           │         │
     ┌─────▼──┐  ┌───▼────────┐
     │ SQLite │  │ Resend API │
     │ local  │  │  (send,    │
     │ store  │  │  receive,  │
     │        │  │  domains)  │
     └────────┘  └────────────┘
```

## Commands

### Core Email

| Command | What it does |
|---|---|
| `send` | Send email (--reply-to-msg for threading, --cc, --bcc, --attach) |
| `reply <id>` | Reply with proper threading headers (--all for Reply All) |
| `forward <id>` | Forward a message (--to, --cc, --text for preamble) |
| `sync` | Pull sent and received messages into local store |
| `inbox ls` | List messages (filter by account, unread status) |
| `inbox read <id>` | Read a message, optionally mark as read |
| `inbox search <q>` | Search messages by keyword |
| `inbox delete <id>` | Delete a message |
| `inbox archive <id>` | Archive a message |
| `draft create` | Save a local draft with attachment snapshots |
| `draft list` | List all drafts |
| `draft show <id>` | View draft details |
| `draft edit <id>` | Edit a draft |
| `draft send <id>` | Send a draft and delete it |
| `draft delete <id>` | Delete a draft |
| `attachments list <id>` | List attachments on a message |
| `attachments get <id> <att>` | Download an attachment to disk |

### Account Management

| Command | What it does |
|---|---|
| `profile add <name>` | Add or update a Resend API profile |
| `profile list` | List all profiles |
| `profile test <name>` | Test API connectivity |
| `account add <email>` | Register an email identity |
| `account list` | List all accounts |
| `account use <email>` | Set the default sending account |
| `signature set <account>` | Set per-account signature |
| `signature show <account>` | Show signature |

### Resend Management

| Command | What it does |
|---|---|
| `domain list` | List your Resend domains |
| `domain get <id>` | Get domain details with DNS records |
| `domain create --name <d>` | Register a new domain |
| `domain verify <id>` | Trigger domain verification |
| `domain delete <id>` | Delete a domain |
| `domain update <id>` | Update tracking settings |
| `audience list` | List audiences |
| `audience create --name <n>` | Create an audience |
| `audience delete <id>` | Delete an audience |
| `contact list --audience <id>` | List contacts in an audience |
| `contact create` | Create a contact |
| `contact update` | Update a contact |
| `contact delete` | Delete a contact |
| `batch send --file <path>` | Send batch emails from a JSON file |
| `api-key list` | List API keys |
| `api-key create --name <n>` | Create an API key |
| `api-key delete <id>` | Delete an API key |

### Agent Tooling

| Command | What it does |
|---|---|
| `agent-info` | Machine-readable JSON capability manifest |
| `skill install` | Install skill file for Claude/Codex/Gemini |
| `skill status` | Check skill installation status |
| `completions <shell>` | Generate shell completions (bash/zsh/fish) |

## Email Threading

Every outgoing email gets a unique `Message-ID` header (`<uuid@yourdomain.com>`). Replies set `In-Reply-To` and `References` per RFC 5322, so threads display correctly in Gmail, Outlook, and Apple Mail.

| Action | Headers Set | Threading |
|---|---|---|
| `send` | Message-ID | New conversation |
| `send --reply-to-msg <id>` | Message-ID, In-Reply-To, References | Threads into existing conversation |
| `reply <id>` | Message-ID, In-Reply-To, References | Threads into existing conversation |
| `reply <id> --all` | Same + preserves CC | Threads with all recipients |
| `forward <id>` | Message-ID only | New conversation (per RFC) |

## Agent Integration

Email CLI follows the [agent-cli-framework](https://github.com/199-biotechnologies/agent-cli-framework) patterns. Any agent that speaks structured JSON can use it.

### Capability Discovery

```bash
email-cli agent-info
```

Returns a JSON manifest of every command, flag, exit code, and output format. An agent calls this once and works from memory.

### Structured Output

All commands produce JSON when piped or when you pass `--json`:

```json
{
  "version": "1",
  "status": "success",
  "data": { ... }
}
```

Errors include actionable suggestions:

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

### Semantic Exit Codes

| Code | Meaning | Agent action |
|---|---|---|
| 0 | Success | Continue |
| 1 | Transient error (network) | Retry |
| 2 | Configuration error | Fix setup |
| 3 | Bad input | Fix arguments |
| 4 | Rate limited | Wait and retry |

### Skill Self-Install

```bash
email-cli skill install
```

Writes a skill file to `~/.claude/skills/email-cli/`, `~/.codex/skills/email-cli/`, and `~/.gemini/skills/email-cli/`. The skill tells agents the CLI exists and to run `agent-info` for full details.

## Configuration

### Local State

All data lives in `~/.local/share/email-cli/email-cli.db` (override with `--db <path>`). Sibling directories:

- `draft-attachments/` -- snapshots of files attached to drafts
- `downloads/` -- fetched attachments (configurable via `--output`)

### Database Tables

| Table | Purpose |
|---|---|
| `profiles` | API key storage |
| `accounts` | Email identities with signatures |
| `messages` | Sent and received email with full metadata |
| `drafts` | Local drafts with attachment snapshots |
| `attachments` | Attachment metadata and local file paths |
| `sync_state` | Per-account sync cursor positions |

SQLite runs with WAL mode, busy timeout, and foreign keys enabled.

### Security

- API keys live in the local SQLite database. Treat `email-cli.db` as sensitive.
- Use `--api-key-env VAR_NAME` or `--api-key-file path` instead of passing keys directly.
- Attachment filenames are sanitized before writing to disk.
- Each send includes a UUID `Idempotency-Key` header.

### Requirements

- A [Resend](https://resend.com) API key with sending enabled
- A verified Resend domain (enable receiving on the domain for inbox sync)
- Rust 1.85+ if building from source (edition 2024)

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT -- see [LICENSE](LICENSE).

---
<div align="center">

Built by [Boris Djordjevic](https://github.com/longevityboris) at [199 Biotechnologies](https://github.com/199-biotechnologies) | [Paperfoot AI](https://paperfoot.ai)

<br />

**If this is useful to you:**

[![Star this repo](https://img.shields.io/github/stars/199-biotechnologies/email-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/199-biotechnologies/email-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

</div>
