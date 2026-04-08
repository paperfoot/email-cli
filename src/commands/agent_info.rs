use crate::output::Format;
use serde_json::json;

pub fn run(_format: Format) {
    let info = json!({
        "name": "email-cli",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-friendly email CLI for Resend",

        "workflow": {
            "setup": "profile add <name> --api-key <key> → account add <email> --profile <name>",
            "check_email": "sync [--account <email>] → inbox list [--account <email>]",
            "send_email": "send --to <addr> --subject <subj> --text <body> [--account <from>]",
            "note": "inbox list reads from local DB. Run sync first to fetch new messages from Resend.",
        },

        "commands": {
            "setup": {
                "profile add <name>": "Add Resend API profile (--api-key, --api-key-env, or --api-key-file)",
                "profile list | ls": "List configured profiles",
                "profile test <name>": "Test profile by listing domains",
                "account add <email>": "Register email account (--profile, --name, --default)",
                "account list | ls": "List configured accounts",
                "account use <email>": "Set the default sending account",
            },
            "compose": {
                "send": "Send email (--to, --subject, --text/--html, --cc, --bcc, --attach, --reply-to-msg <id> for threading)",
                "reply <message_id>": "Reply to a message (--all for Reply All, --text/--html, --attach)",
                "forward <message_id>": "Forward a message (--to, --cc, --bcc, --text for preamble)",
                "draft create | new": "Create local draft (same flags as send, plus --reply-to)",
                "draft list | ls": "List drafts (--account)",
                "draft show <id>": "Show draft content",
                "draft edit <id>": "Edit draft (--subject, --text, --html, --to, --cc, --bcc)",
                "draft send <id>": "Send a draft",
                "draft delete | rm <id>": "Delete a draft",
                "batch send --file <path>": "Send batch emails from JSON file",
            },
            "inbox": {
                "sync": "Fetch new messages from Resend into local DB (--account, --limit, --watch, --interval, --notify)",
                "inbox sync": "Convenience alias for sync (--account, --limit)",
                "inbox list | ls": "List messages (--account, --limit, --unread, --archived, --after <cursor>). Pagination: --after <id> returns {messages, has_more, next_cursor}",
                "inbox read <id>": "Read message content (--mark-read, --no-mark-read, --raw)",
                "inbox mark <ids...>": "Mark messages as read or unread (--read or --unread, mutually exclusive)",
                "inbox search <query>": "Search messages (--account, --limit)",
                "inbox delete | rm <ids...>": "Delete one or more messages (bulk: pass multiple IDs)",
                "inbox archive <ids...>": "Archive one or more messages (bulk: pass multiple IDs)",
                "inbox unarchive <ids...>": "Unarchive one or more messages (bulk: pass multiple IDs)",
                "inbox thread <id>": "Show all messages in a conversation thread",
                "inbox purge --before <date>": "Delete messages older than YYYY-MM-DD (--account)",
            },
            "attachments": {
                "attachments list | ls <message_id>": "List attachments on a message",
                "attachments get | show <message_id> <attachment_id>": "Download attachment (--output)",
            },
            "signatures": {
                "signature set <account>": "Set signature (--text)",
                "signature show <account>": "Show current signature",
            },
            "domains": {
                "domain list | ls": "List domains",
                "domain get | show <id>": "Get domain details and DNS records",
                "domain create | new --name <domain>": "Register a new domain (--region)",
                "domain verify <id>": "Trigger domain verification",
                "domain delete | rm <id>": "Delete a domain",
                "domain update <id>": "Update tracking settings (--open-tracking, --click-tracking)",
            },
            "contacts": {
                "contact list | ls": "List contacts (--limit 1-100, --after <id> for cursor pagination)",
                "contact get | show <id_or_email>": "Get a contact by id or email (surfaces properties)",
                "contact create | new --email <email>": "Create a contact (--first-name, --last-name, --unsubscribed, --properties '{\"k\":\"v\"}', --segments seg1,seg2, --topics top_xxx:opt_in,top_yyy:opt_out). Property keys must be defined via `contact-property create` first.",
                "contact update <id_or_email>": "Update contact fields (--first-name, --last-name, --unsubscribed, --properties)",
                "contact delete | rm <id_or_email>": "Delete a contact",
            },
            "segments": {
                "segment list | ls": "List segments (replaces deprecated `audience` noun)",
                "segment get | show <id>": "Get segment details",
                "segment create | new --name <name>": "Create a segment",
                "segment delete | rm <id>": "Delete a segment",
                "segment contact-add --contact <id_or_email> --segment <id>": "Add a contact to a segment",
                "segment contact-remove --contact <id_or_email> --segment <id>": "Remove a contact from a segment",
                "segment contact-list --contact <id_or_email>": "List the segments a contact belongs to",
                "segment contacts <id>": "List contacts in a segment",
            },
            "contact_properties": {
                "contact-property list | ls": "List defined contact-property schemas",
                "contact-property get | show <id>": "Get a contact-property schema",
                "contact-property create | new --key <k>": "Define a contact-property (--property-type string|number, --fallback). Required before assigning values via `contact create --properties`.",
                "contact-property update <id>": "Update a contact-property's fallback value (--fallback, --property-type)",
                "contact-property delete | rm <id>": "Delete a contact-property schema",
            },
            "topics": {
                "topic list | ls": "List topics",
                "topic get | show <id>": "Get topic details",
                "topic create | new --name <name>": "Create a topic (--description, --default-subscription opt_in|opt_out, --visibility public|private)",
                "topic update <id>": "Update a topic (--name, --description, --default-subscription, --visibility)",
                "topic delete | rm <id>": "Delete a topic",
                "topic contact-set --contact <id_or_email> --topic <id> --subscription opt_in|opt_out": "Subscribe or unsubscribe a contact to a topic. Used by broadcasts for granular preferences.",
                "topic contact-list --contact <id_or_email>": "List a contact's topic subscriptions",
            },
            "broadcasts": {
                "broadcast list | ls": "List broadcasts",
                "broadcast get | show <id>": "Get broadcast details (segment_id, topic_id, html, text, preview_text, status)",
                "broadcast create | new --segment-id <id> --from <addr> --subject <subj>": "Create a broadcast (--html, --text, --name, --reply-to, --topic-id, --scheduled-at, --send for inline send). Use `{{{RESEND_UNSUBSCRIBE_URL}}}` in --html for auto unsubscribe wiring. --audience-id is accepted as a legacy alias for --segment-id.",
                "broadcast update <id>": "Update a draft broadcast (--segment-id, --from, --subject, --html, --text, --name, --reply-to, --topic-id)",
                "broadcast send <id>": "Send (or schedule) a broadcast (--scheduled-at <iso>)",
                "broadcast delete | rm <id>": "Delete (or cancel scheduled) broadcast",
            },
            "delivery": {
                "outbox list | ls": "List pending outbox items",
                "outbox retry <id>": "Retry a failed send",
                "outbox flush": "Retry all pending items",
                "events list | ls": "View delivery events from the local DB (--message <id>, --limit). Populated by `webhook listen`.",
                "email list | ls": "List sent emails via Resend GET /emails (--limit 1-100, --after <id>). Each row includes last_event for poll-based delivery checks.",
                "webhook listen": "Start webhook listener (--port, default 8080)",
            },
            "api_keys": {
                "api-key list | ls": "List API keys",
                "api-key create | new --name <name>": "Create key (--permission: full-access|sending-access)",
                "api-key delete | rm <id>": "Delete an API key",
            },
            "daemon": {
                "daemon": "Run as menu bar daemon with sync and notifications (--account, --interval)",
            },
            "meta": {
                "update": "Self-update from GitHub Releases (--check to check only)",
                "agent-info": "This manifest",
                "skill install": "Install skill file to agent platforms",
                "skill status": "Check skill installation status",
                "completions <shell>": "Generate shell completions (bash, zsh, fish)",
            },
        },

        "aliases": "All CRUD subcommands accept both long and short forms: list/ls, delete/rm, create/new, get/show",

        "flags": {
            "--json": "Force JSON output (auto-enabled when piped)",
            "--db <path>": "Custom database path",
        },

        "exit_codes": {
            "0": "Success (including --help and --version)",
            "1": "Transient error (network, IO) — retry",
            "2": "Configuration error — fix setup",
            "3": "Bad input — fix arguments",
            "4": "Rate limited — wait and retry",
        },

        "envelope": {
            "version": "1",
            "success_shape": "{ version, status, data }",
            "error_shape": "{ version, status, error: { code, message, suggestion } }",
        },

        "auto_json_when_piped": true,
        "env_prefix": "EMAIL_CLI_",
    });
    println!("{}", serde_json::to_string_pretty(&info).unwrap());
}
