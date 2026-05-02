<div align="center">

# Email CLI

**Send, receive, and manage email from your terminal. Built for AI agents.**

<br />

[![Star this repo](https://img.shields.io/github/stars/paperfoot/email-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/email-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

<br />

[![Crates.io](https://img.shields.io/crates/v/email-cli?style=for-the-badge&logo=rust&logoColor=white&label=crates.io)](https://crates.io/crates/email-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Homebrew](https://img.shields.io/badge/Homebrew-tap-FBB040?style=for-the-badge&logo=homebrew&logoColor=white)](https://github.com/paperfoot/homebrew-tap)

---

A single binary that gives your AI agent a real email address. Send, receive, reply, draft, sync, and manage contacts, broadcasts, segments, and topics through Resend -- all from the command line. No IMAP. No browser inbox. No MCP server. Just a local SQLite mailbox and a command surface agents discover at runtime.

Works inside any agent harness that can invoke a CLI: Claude Code, Cursor, Warp, Codex, Gemini CLI, plain shell scripts. One Resend API key is the only external requirement.

Pair it with [**Minimail**](https://github.com/paperfoot/minimail-mac) — a macOS menu bar GUI that shells out to this CLI — if you also want a five-second visual peek at the inbox.

[Why](#why) | [Install](#install) | [How It Works](#how-it-works) | [Commands](#commands) | [Agent Integration](#agent-integration) | [Companion App](#companion-app-minimail) | [Configuration](#configuration) | [Contributing](#contributing)

</div>

## Why

AI agents need email. The existing options are bad:

- **IMAP/SMTP** requires complex server configuration, credential management, and connection handling. Agents struggle with it.
- **Email APIs** work for sending, but agents need a local mailbox to track threads, drafts, and read state.
- **Browser-based inboxes** are not scriptable. Agents cannot use them.
- **MCP servers** add a network hop and a config burden per harness. Overkill when a signed binary already works.

Email CLI wraps the [Resend API](https://resend.com) in a local-first CLI. Your agent gets a verified email address, a local SQLite mailbox, and structured JSON output with semantic exit codes. It calls `agent-info` once to learn every command, then works from memory.

It works just as well for humans — and pairs with [Minimail](https://github.com/paperfoot/minimail-mac) if you want a GUI.

## Install

### One-line install (pre-built binary, no Rust required)

```bash
curl -fsSL https://raw.githubusercontent.com/paperfoot/email-cli/main/install.sh | sh
```

### Homebrew

```bash
brew tap paperfoot/tap
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

# 2. Register a sending identity
email-cli account add agent@yourdomain.com \
  --profile default \
  --name "Agent" \
  --default

# 3. Send an email
email-cli send \
  --to someone@example.com \
  --subject "Hello from email-cli" \
  --text "Sent from the terminal."

# 4. Sync and read incoming mail
email-cli sync
email-cli inbox ls
email-cli inbox read 1 --mark-read

# 5. Reply (threads correctly with In-Reply-To + References)
email-cli reply 1 --text "Got it, thanks."
```

## How It Works

Three concepts:

1. **Profile** -- a Resend API key. You can have multiple profiles for different Resend accounts.
2. **Account** -- a sender/receiver identity (`agent@yourdomain.com`). Each account belongs to a profile.
3. **Local mailbox** -- a SQLite database that stores messages, drafts, attachments, and sync cursors.

Resend handles delivery. Email CLI handles the local operating model: read tracking, threading, drafts, batch sends, and structured output.

```
┌────────────────────────────────┐
│         Your Agent / You       │
│    (Claude, Codex, Gemini)     │
└──────────────┬─────────────────┘
               │  CLI commands
               ▼
┌────────────────────────────────┐
│          email-cli             │
│   structured JSON output,      │
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

Every outgoing email gets a unique `Message-ID`; `reply` and `send --reply-to-msg` set `In-Reply-To` and `References` per RFC 5322, so threads display correctly in Gmail, Outlook, and Apple Mail.

## Menu Bar Daemon (macOS)

`email-cli daemon` runs in the background, syncs your inbox on a timer, and shows unread count in the menu bar. Native `UNUserNotificationCenter` banners fire on incoming mail -- including from `sync --notify` and `webhook listen --notify`, even if the daemon isn't running.

```bash
email-cli daemon                      # foreground, default 60s interval
email-cli daemon --interval 30        # 30s poll
email-cli daemon --account you@x.com  # single account
```

### Run at login

```bash
email-cli autostart install           # installs ~/Library/LaunchAgents/ai.paperfoot.email-cli.daemon.plist
email-cli autostart status            # check if loaded
email-cli autostart uninstall         # remove
```

The LaunchAgent loads immediately (no reboot needed) and restarts the daemon automatically if it exits unexpectedly. Logs go to `/tmp/email-cli-daemon.log`.

### How notifications work

The first time a notification fires, email-cli extracts an embedded codesigned `.app` bundle to `~/Library/Application Support/email-cli/EmailCLI.app/`. macOS 26+ requires this bundle for `UNUserNotificationCenter` to work -- raw binaries fail `TCC` silently. If the bundle can't be extracted, email-cli falls back to `osascript`.

## Commands

Email CLI groups its commands by area:

- **Email** -- `send`, `reply`, `forward`, `sync`, `inbox`, `draft`, `attachments`
- **Identity** -- `profile`, `account`, `signature`, `domain`, `api-key`
- **Audience (mailing lists)** -- `contact`, `segment`, `contact-property`, `topic`, `broadcast`
- **Delivery** -- `outbox` (durable send queue), `webhook listen`, `events`, `email list`, `batch send`
- **Daemon** -- `daemon`, `autostart` (macOS menu bar, native notifications, LaunchAgent autostart)
- **Agent tooling** -- `agent-info`, `skill install`, `completions`
- **Ops** -- `update` (self-update), `log` (command audit trail)

The canonical, always-current reference -- every command, subcommand alias, flag, and exit code -- lives in the CLI itself:

```bash
email-cli agent-info
```

> **Audience primitives:** Resend renamed "Audiences" to **Segments** in November 2025. Contacts are flat -- each contact can belong to zero, one, or multiple segments. **Topics** drive granular per-contact subscription preferences for broadcasts. **Contact properties** are typed custom fields for merge tags. Use `segment`, `topic`, and `contact-property` respectively.

## Agent Integration

Email CLI follows the [agent-cli-framework](https://github.com/paperfoot/agent-cli-framework) patterns. Any agent that speaks structured JSON can use it.

### Capability Discovery

```bash
email-cli agent-info
```

Returns a JSON manifest of every command, flag, exit code, and output format. An agent calls this once and works from memory.

### Structured Output

All commands produce JSON when piped or when you pass `--json`:

```json
{ "version": "1", "status": "success", "data": { ... } }
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

## Companion App: Minimail

Email CLI is the agent-facing half of a two-component product. The human-facing half is [**Minimail**](https://github.com/paperfoot/minimail-mac) — a macOS 26 menu bar app that shells out to this CLI for every operation.

|  | Email CLI | Minimail |
|---|---|---|
| Interface | Terminal, structured JSON | SwiftUI popover, 420×580 |
| Audience | AI agents, automation, scripting | Humans who want a quick visual peek |
| Platforms | macOS, Linux, Windows | macOS 26 only |
| License | MIT | Proprietary (paid app, coming soon) |
| Required? | Yes, for everything | No — purely optional |

Minimail makes no network calls of its own; it runs `email-cli send ... --json` and friends, then renders the output. Every feature in the GUI maps to a subcommand here. Install either, both, or neither — the CLI is the product and the GUI is a convenience layer.

## Configuration

### Local State

All data lives in `~/.local/share/email-cli/email-cli.db` (override with `--db <path>`). Sibling directories:

- `draft-attachments/` -- snapshots of files attached to drafts
- `sent-attachments/` -- snapshots of files attached to messages sent from this machine
- `downloads/` -- fetched attachments (use `attachments get --output-dir`, legacy `--output`, or exact `--output-file`)

SQLite runs with WAL mode, busy timeout, and foreign keys enabled.

### Security

- API keys live in the local SQLite database. Treat `email-cli.db` as sensitive.
- Prefer `--api-key-env VAR_NAME` or `--api-key-file path` over passing keys directly.
- Attachment filenames are sanitized before writing to disk.
- Every send is written to a durable outbox with a stable `Idempotency-Key` before delivery is attempted; retry with `outbox retry` or `outbox flush`.

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

[![Star this repo](https://img.shields.io/github/stars/paperfoot/email-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/email-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

</div>
