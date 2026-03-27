use crate::output::Format;
use serde_json::json;
use std::collections::BTreeMap;

pub fn run(_format: Format) {
    let mut commands = BTreeMap::new();
    for (k, v) in [
        ("profile add <name>", "Add or update a Resend API profile"),
        ("profile list", "List configured profiles"),
        ("profile test <name>", "Test profile by listing domains"),
        ("account add <email>", "Register an email account under a profile"),
        ("account list", "List configured accounts"),
        ("account use <email>", "Set the default account"),
        ("signature set <account>", "Set signature text for an account"),
        ("signature show <account>", "Show signature for an account"),
        ("send", "Send an email"),
        ("reply <message_id>", "Reply to a received message"),
        ("draft create", "Create a local draft"),
        ("draft list", "List drafts"),
        ("draft show <id>", "Show draft details"),
        ("draft send <id>", "Send a draft"),
        ("sync", "Sync sent and received messages from Resend"),
        ("inbox ls", "List messages"),
        ("inbox read <id>", "Read a message"),
        ("attachments list <message_id>", "List attachments for a message"),
        ("attachments get <message_id> <attachment_id>", "Download an attachment"),
        ("domain list", "List domains for the default profile"),
        ("domain get <id>", "Get domain details and DNS records"),
        ("domain create --name <domain>", "Register a new domain"),
        ("domain verify <id>", "Trigger domain verification"),
        ("domain delete <id>", "Delete a domain"),
        ("domain update <id>", "Update domain tracking settings"),
        ("audience list", "List audiences"),
        ("audience get <id>", "Get audience details"),
        ("audience create --name <name>", "Create an audience"),
        ("audience delete <id>", "Delete an audience"),
        ("contact list --audience <id>", "List contacts in an audience"),
        ("contact get --audience <id> <contact_id>", "Get contact details"),
        ("contact create --audience <id> --email <email>", "Create a contact"),
        ("contact update --audience <id> <contact_id>", "Update a contact"),
        ("contact delete --audience <id> <contact_id>", "Delete a contact"),
        ("batch send --file <path>", "Send batch emails from a JSON file"),
        ("api-key list", "List API keys"),
        ("api-key create --name <name>", "Create an API key"),
        ("api-key delete <id>", "Delete an API key"),
        ("agent-info", "This manifest"),
        ("skill install", "Install skill file to agent platforms"),
        ("skill status", "Check skill installation status"),
        ("completions <shell>", "Generate shell completions"),
    ] {
        commands.insert(k, v);
    }

    let info = json!({
        "name": "email-cli",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-friendly email CLI for Resend",
        "commands": commands,
        "flags": {
            "--json": "Force JSON output (auto-enabled when piped)",
            "--db <path>": "Custom database path",
        },
        "exit_codes": {
            "0": "Success",
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
