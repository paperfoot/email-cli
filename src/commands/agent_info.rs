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
                "send": "Send email (--to, --subject, --text/--html, --cc, --bcc, --attach)",
                "reply <message_id>": "Reply to a message (--text/--html, --attach)",
                "draft create | new": "Create local draft (same flags as send, plus --reply-to)",
                "draft list | ls": "List drafts (--account)",
                "draft show <id>": "Show draft content",
                "draft edit <id>": "Edit draft (--subject, --text, --html, --to, --cc, --bcc)",
                "draft send <id>": "Send a draft",
                "draft delete | rm <id>": "Delete a draft",
                "batch send --file <path>": "Send batch emails from JSON file",
            },
            "inbox": {
                "sync": "Fetch new messages from Resend into local DB (--account, --limit, --watch, --interval)",
                "inbox sync": "Same as sync (convenience alias)",
                "inbox list | ls": "List messages (--account, --limit, --unread, --archived)",
                "inbox read <id>": "Read message content (--mark-read, --raw)",
                "inbox search <query>": "Search messages (--account, --limit)",
                "inbox delete | rm <id>": "Delete a message",
                "inbox archive <id>": "Archive a message",
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
            "audiences_and_contacts": {
                "audience list | ls": "List audiences",
                "audience get | show <id>": "Get audience details",
                "audience create | new --name <name>": "Create an audience",
                "audience delete | rm <id>": "Delete an audience",
                "contact list | ls --audience <id>": "List contacts",
                "contact get | show --audience <id> <contact_id>": "Get contact details",
                "contact create | new --audience <id> --email <email>": "Create contact (--first-name, --last-name)",
                "contact update --audience <id> <contact_id>": "Update contact fields",
                "contact delete | rm --audience <id> <contact_id>": "Delete a contact",
            },
            "delivery": {
                "outbox list | ls": "List pending outbox items",
                "outbox retry <id>": "Retry a failed send",
                "outbox flush": "Retry all pending items",
                "events list | ls": "View delivery events (--message <id>, --limit)",
                "webhook listen": "Start webhook listener (--port, default 8080)",
            },
            "api_keys": {
                "api-key list | ls": "List API keys",
                "api-key create | new --name <name>": "Create key (--permission: full-access|sending-access)",
                "api-key delete | rm <id>": "Delete an API key",
            },
            "meta": {
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
