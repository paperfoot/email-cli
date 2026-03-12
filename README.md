# email-cli

`email-cli` is a local-first email tool for AI agents and terminal-heavy teams.

The idea is simple: if you already use Resend for a verified domain, you should be able to give an agent a clean CLI and let it handle real email without dragging in IMAP or a browser inbox. An agent should be able to check its mailbox, send a reply, save a draft, then move on.

Built by Boris Djordjevic.

## What it does

- manages logical email accounts on a Resend-backed domain
- sends email from any configured alias
- syncs sent and received mail into a local SQLite store
- reads mailbox state from the terminal or as JSON
- saves drafts and sends them later
- replies with `In-Reply-To` and `References`
- applies per-account signatures
- lists and downloads received attachments

This is not an IMAP server and it is not pretending to be one. It is a practical mailbox layer for agents.

## Why this exists

Most email tooling is built for people sitting in front of an inbox UI. Agents need something else. They need a tool they can call over and over without ceremony, with stable JSON output, predictable side effects, and enough local state to stay useful when the network is slow or the upstream API is noisy.

That is `email-cli`.

## How it works

There are three core ideas:

1. `profile`
   A Resend API context.

2. `account`
   A logical sender/receiver identity such as `agent1@yourdomain.com`.

3. `local mailbox store`
   A SQLite database that keeps mailbox state, drafts, sync cursors, attachments, plus message history.

Resend handles delivery and receiving. `email-cli` handles the local operating model that agents actually need.

## Current features

- direct HTTP integration with Resend
- multi-account support
- durable per-account sync cursors
- idempotent outbound send requests
- safe attachment downloads
- draft attachment snapshots
- JSON-first command surface for automation
- local SQLite with WAL mode and busy timeout enabled

## Current limits

- profiles still store the Resend API key in the local SQLite database
- `reply-all` is not implemented yet
- threaded replies to sent messages are intentionally blocked until sent RFC `Message-ID` handling is added
- first sync on a busy mailbox can take longer because it now pages safely instead of sampling a small recent window
- there is no background daemon or webhook ingester in this repo yet

## Requirements

- Rust toolchain
- a Resend API key with mailbox endpoint access
- a verified Resend domain
- receiving enabled on the domain if you want inbox sync

Using `--api-key-file` or `--api-key-env` is safer than passing `--api-key` directly.

## Build

```bash
cargo build
```

## Quick start

Add a profile from an env file:

```bash
./target/debug/email-cli profile add local \
  --api-key-file /path/to/.env \
  --api-key-name RESEND_API_KEY
```

Check the domains available on that profile:

```bash
./target/debug/email-cli profile test local
```

Add two logical accounts:

```bash
./target/debug/email-cli account add agent1@yourdomain.com \
  --profile local \
  --name "Agent 1" \
  --default

./target/debug/email-cli account add agent2@yourdomain.com \
  --profile local \
  --name "Agent 2"
```

Set a signature:

```bash
./target/debug/email-cli signature set agent1@yourdomain.com --text $'Regards,\nAgent 1'
```

Send a message:

```bash
./target/debug/email-cli send \
  --account agent1@yourdomain.com \
  --to agent2@yourdomain.com \
  --subject "Hello" \
  --text "Testing email-cli"
```

Sync and read mail:

```bash
./target/debug/email-cli sync --account agent2@yourdomain.com --limit 20
./target/debug/email-cli inbox ls --account agent2@yourdomain.com
./target/debug/email-cli inbox read 2
```

Create and send a draft:

```bash
./target/debug/email-cli draft create \
  --account agent2@yourdomain.com \
  --to agent1@yourdomain.com \
  --subject "Re: Hello" \
  --text "Draft reply" \
  --reply-to-message-id 2

./target/debug/email-cli draft list --account agent2@yourdomain.com
./target/debug/email-cli draft send <draft-id>
```

Reply directly:

```bash
./target/debug/email-cli reply 4 --text "Following up"
```

Work with attachments:

```bash
./target/debug/email-cli attachments list 2
./target/debug/email-cli attachments get 2 <attachment-id> --output ./downloads
```

Add `--json` to any command when the caller is an agent.

## Example use cases

- give each agent its own address on a shared domain
- let support agents read and answer inbound email from the terminal
- let research or ops agents send updates without logging into a web inbox
- use email as a simple coordination layer between agents
- build a thin MCP wrapper on top of the CLI later if you want stricter tool contracts

## Design notes

The project aims for a narrow, durable surface:

- one binary
- local state
- predictable commands
- easy scripting
- no fake mailbox abstractions

That tradeoff keeps the tool useful in practice. It also keeps the path open for a future shared mode with webhook ingestion or a thin server if the product grows beyond one machine.

## Development status

The current implementation has been exercised live against a real Resend setup, including:

- sending
- receiving
- syncing
- drafts
- replies
- signatures
- attachment download
- account-isolation checks

There is still work to do before calling it fully production-ready, mostly around secret storage, automated tests, and richer reply semantics.

## Security notes

- treat the local SQLite database as sensitive
- do not commit API keys or `.env` files
- attachment filenames are sanitized before local writes
- outbound sends use idempotency keys to reduce duplicate-send risk

## Roadmap

- encrypted or keychain-backed secret storage
- `reply-all`
- automated integration tests
- optional webhook-assisted ingestion
- richer mailbox search and thread views

## License

No license has been added yet.
