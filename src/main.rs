use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, SecondsFormat, Utc};
use clap::{Args, Parser, Subcommand};
use dirs::data_local_dir;
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "email-cli",
    version,
    about = "Agent-friendly email CLI for Resend"
)]
struct Cli {
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    Signature {
        #[command(subcommand)]
        command: SignatureCommand,
    },
    Send(SendArgs),
    Reply(ReplyArgs),
    Draft {
        #[command(subcommand)]
        command: DraftCommand,
    },
    Sync(SyncArgs),
    Inbox {
        #[command(subcommand)]
        command: InboxCommand,
    },
    Attachments {
        #[command(subcommand)]
        command: AttachmentsCommand,
    },
}

#[derive(Subcommand)]
enum ProfileCommand {
    Add(ProfileAddArgs),
    List,
    Test(ProfileTestArgs),
}

#[derive(Args)]
struct ProfileAddArgs {
    name: String,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long)]
    api_key_env: Option<String>,
    #[arg(long)]
    api_key_file: Option<PathBuf>,
    #[arg(long, default_value = "RESEND_API_KEY")]
    api_key_name: String,
}

#[derive(Args)]
struct ProfileTestArgs {
    name: String,
}

#[derive(Subcommand)]
enum AccountCommand {
    Add(AccountAddArgs),
    List,
    Use(AccountUseArgs),
}

#[derive(Args)]
struct AccountAddArgs {
    email: String,
    #[arg(long)]
    profile: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long, default_value = "")]
    signature: String,
    #[arg(long)]
    default: bool,
}

#[derive(Args)]
struct AccountUseArgs {
    email: String,
}

#[derive(Subcommand)]
enum SignatureCommand {
    Set(SignatureSetArgs),
    Show(SignatureShowArgs),
}

#[derive(Args)]
struct SignatureSetArgs {
    account: String,
    #[arg(long)]
    text: String,
}

#[derive(Args)]
struct SignatureShowArgs {
    account: String,
}

#[derive(Args, Clone)]
struct ComposeArgs {
    #[arg(long)]
    account: Option<String>,
    #[arg(long = "to", required = true)]
    to: Vec<String>,
    #[arg(long = "cc")]
    cc: Vec<String>,
    #[arg(long = "bcc")]
    bcc: Vec<String>,
    #[arg(long)]
    subject: String,
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    text_file: Option<PathBuf>,
    #[arg(long)]
    html: Option<String>,
    #[arg(long)]
    html_file: Option<PathBuf>,
    #[arg(long = "attach")]
    attachments: Vec<PathBuf>,
}

#[derive(Args)]
struct SendArgs {
    #[command(flatten)]
    compose: ComposeArgs,
}

#[derive(Args)]
struct ReplyArgs {
    message_id: i64,
    #[arg(long)]
    account: Option<String>,
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    text_file: Option<PathBuf>,
    #[arg(long)]
    html: Option<String>,
    #[arg(long)]
    html_file: Option<PathBuf>,
    #[arg(long = "attach")]
    attachments: Vec<PathBuf>,
}

#[derive(Subcommand)]
enum DraftCommand {
    Create(DraftCreateArgs),
    List(DraftListArgs),
    Show(DraftShowArgs),
    Send(DraftSendArgs),
}

#[derive(Args)]
struct DraftCreateArgs {
    #[command(flatten)]
    compose: ComposeArgs,
    #[arg(long)]
    reply_to_message_id: Option<i64>,
}

#[derive(Args)]
struct DraftListArgs {
    #[arg(long)]
    account: Option<String>,
}

#[derive(Args)]
struct DraftShowArgs {
    id: String,
}

#[derive(Args)]
struct DraftSendArgs {
    id: String,
}

#[derive(Args)]
struct SyncArgs {
    #[arg(long)]
    account: Option<String>,
    #[arg(long, default_value_t = 25)]
    limit: usize,
}

#[derive(Subcommand)]
enum InboxCommand {
    Ls(InboxListArgs),
    Read(InboxReadArgs),
}

#[derive(Args)]
struct InboxListArgs {
    #[arg(long)]
    account: Option<String>,
    #[arg(long, default_value_t = 20)]
    limit: usize,
    #[arg(long)]
    unread: bool,
}

#[derive(Args)]
struct InboxReadArgs {
    id: i64,
    #[arg(long, default_value_t = true)]
    mark_read: bool,
}

#[derive(Subcommand)]
enum AttachmentsCommand {
    List(AttachmentListArgs),
    Get(AttachmentGetArgs),
}

#[derive(Args)]
struct AttachmentListArgs {
    message_id: i64,
}

#[derive(Args)]
struct AttachmentGetArgs {
    message_id: i64,
    attachment_id: String,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct ProfileRecord {
    name: String,
    created_at: String,
}

#[derive(Debug, Serialize, Clone)]
struct AccountRecord {
    email: String,
    profile_name: String,
    display_name: Option<String>,
    signature: String,
    is_default: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
struct MessageRecord {
    id: i64,
    remote_id: String,
    direction: String,
    account_email: String,
    from_addr: String,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    reply_to: Vec<String>,
    subject: String,
    text_body: Option<String>,
    html_body: Option<String>,
    rfc_message_id: Option<String>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    last_event: Option<String>,
    is_read: bool,
    created_at: String,
    synced_at: String,
}

#[derive(Debug, Serialize, Clone)]
struct DraftRecord {
    id: String,
    account_email: String,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    text_body: Option<String>,
    html_body: Option<String>,
    reply_to_message_id: Option<i64>,
    attachment_paths: Vec<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
struct AttachmentRecord {
    id: i64,
    message_id: i64,
    remote_attachment_id: Option<String>,
    filename: Option<String>,
    content_type: Option<String>,
    size: Option<i64>,
    download_url: Option<String>,
    local_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct SyncSummary {
    profiles: usize,
    sent_messages: usize,
    received_messages: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct DomainList {
    #[serde(default)]
    data: Vec<Domain>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Domain {
    name: String,
    status: Option<String>,
    region: Option<String>,
    capabilities: Option<DomainCapabilities>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DomainCapabilities {
    sending: Option<String>,
    receiving: Option<String>,
}

#[derive(Debug, Serialize)]
struct SendEmailRequest {
    from: String,
    to: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cc: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bcc: Vec<String>,
    subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<SendAttachment>,
}

#[derive(Debug, Serialize)]
struct SendAttachment {
    filename: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct SendEmailResponse {
    id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ListResponse<T> {
    #[serde(default)]
    data: Vec<T>,
    has_more: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct SentEmail {
    id: String,
    from: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    to: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    cc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    bcc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    reply_to: Vec<String>,
    subject: Option<String>,
    created_at: Option<String>,
    last_event: Option<String>,
    html: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct ReceivedEmail {
    id: String,
    from: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    to: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    cc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    bcc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    reply_to: Vec<String>,
    subject: Option<String>,
    created_at: Option<String>,
    message_id: Option<String>,
    html: Option<String>,
    text: Option<String>,
    #[serde(default)]
    attachments: Vec<ReceivedAttachment>,
    headers: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct ReceivedAttachment {
    id: Option<String>,
    filename: Option<String>,
    #[serde(alias = "contentType")]
    content_type: Option<String>,
    size: Option<i64>,
    #[serde(alias = "downloadUrl")]
    download_url: Option<String>,
}

struct App {
    conn: Connection,
    db_path: PathBuf,
    json: bool,
}

struct ResendClient {
    client: Client,
    api_key: String,
}

#[derive(Clone)]
struct ResolvedCompose {
    account: AccountRecord,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    text: Option<String>,
    html: Option<String>,
    attachments: Vec<PathBuf>,
}

#[derive(Clone)]
struct ReplyHeaders {
    in_reply_to: Option<String>,
    references: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or(default_db_path()?);
    let app = App::new(db_path, cli.json)?;

    match cli.command {
        Command::Profile { command } => match command {
            ProfileCommand::Add(args) => app.profile_add(args),
            ProfileCommand::List => app.profile_list(),
            ProfileCommand::Test(args) => app.profile_test(args),
        },
        Command::Account { command } => match command {
            AccountCommand::Add(args) => app.account_add(args),
            AccountCommand::List => app.account_list(),
            AccountCommand::Use(args) => app.account_use(args),
        },
        Command::Signature { command } => match command {
            SignatureCommand::Set(args) => app.signature_set(args),
            SignatureCommand::Show(args) => app.signature_show(args),
        },
        Command::Send(args) => app.send(args),
        Command::Reply(args) => app.reply(args),
        Command::Draft { command } => match command {
            DraftCommand::Create(args) => app.draft_create(args),
            DraftCommand::List(args) => app.draft_list(args),
            DraftCommand::Show(args) => app.draft_show(args),
            DraftCommand::Send(args) => app.draft_send(args),
        },
        Command::Sync(args) => app.sync(args),
        Command::Inbox { command } => match command {
            InboxCommand::Ls(args) => app.inbox_list(args),
            InboxCommand::Read(args) => app.inbox_read(args),
        },
        Command::Attachments { command } => match command {
            AttachmentsCommand::List(args) => app.attachments_list(args),
            AttachmentsCommand::Get(args) => app.attachments_get(args),
        },
    }
}

impl App {
    fn new(db_path: PathBuf, json: bool) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open {}", db_path.display()))?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS profiles (
                name TEXT PRIMARY KEY,
                api_key TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS accounts (
                email TEXT PRIMARY KEY,
                profile_name TEXT NOT NULL REFERENCES profiles(name),
                display_name TEXT,
                signature TEXT NOT NULL DEFAULT '',
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_default
            ON accounts(is_default) WHERE is_default = 1;

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                remote_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                account_email TEXT NOT NULL REFERENCES accounts(email),
                from_addr TEXT NOT NULL,
                to_json TEXT NOT NULL,
                cc_json TEXT NOT NULL DEFAULT '[]',
                bcc_json TEXT NOT NULL DEFAULT '[]',
                reply_to_json TEXT NOT NULL DEFAULT '[]',
                subject TEXT NOT NULL DEFAULT '',
                text_body TEXT,
                html_body TEXT,
                rfc_message_id TEXT,
                in_reply_to TEXT,
                references_json TEXT NOT NULL DEFAULT '[]',
                last_event TEXT,
                is_read INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                synced_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                raw_json TEXT NOT NULL,
                UNIQUE(remote_id, direction, account_email)
            );

            CREATE TABLE IF NOT EXISTS drafts (
                id TEXT PRIMARY KEY,
                account_email TEXT NOT NULL REFERENCES accounts(email),
                to_json TEXT NOT NULL,
                cc_json TEXT NOT NULL DEFAULT '[]',
                bcc_json TEXT NOT NULL DEFAULT '[]',
                subject TEXT NOT NULL DEFAULT '',
                text_body TEXT,
                html_body TEXT,
                reply_to_message_id INTEGER REFERENCES messages(id),
                attachment_paths_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS attachments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                remote_attachment_id TEXT,
                filename TEXT,
                content_type TEXT,
                size INTEGER,
                download_url TEXT,
                local_path TEXT,
                raw_json TEXT NOT NULL,
                UNIQUE(message_id, remote_attachment_id, filename)
            );

            CREATE TABLE IF NOT EXISTS sync_state (
                account_email TEXT NOT NULL REFERENCES accounts(email) ON DELETE CASCADE,
                direction TEXT NOT NULL,
                cursor_id TEXT,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (account_email, direction)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_account_created
            ON messages(account_email, created_at DESC);

            CREATE INDEX IF NOT EXISTS idx_messages_account_unread_created
            ON messages(account_email, is_read, created_at DESC);

            CREATE INDEX IF NOT EXISTS idx_drafts_account_updated
            ON drafts(account_email, updated_at DESC);

            CREATE INDEX IF NOT EXISTS idx_attachments_message
            ON attachments(message_id);
            ",
        )?;

        Ok(Self {
            conn,
            db_path,
            json,
        })
    }

    fn profile_add(&self, args: ProfileAddArgs) -> Result<()> {
        let api_key = resolve_api_key(
            args.api_key,
            args.api_key_env,
            args.api_key_file,
            &args.api_key_name,
        )?;

        self.conn.execute(
            "
            INSERT INTO profiles (name, api_key, updated_at)
            VALUES (?1, ?2, CURRENT_TIMESTAMP)
            ON CONFLICT(name) DO UPDATE SET
                api_key = excluded.api_key,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![args.name, api_key],
        )?;

        if self.json {
            print_json(&json!({
                "name": args.name,
                "status": "saved",
                "db_path": self.db_path.display().to_string(),
            }))?;
        } else {
            println!("saved profile {}", args.name);
        }

        Ok(())
    }

    fn profile_list(&self) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, created_at FROM profiles ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            Ok(ProfileRecord {
                name: row.get(0)?,
                created_at: row.get(1)?,
            })
        })?;
        let profiles = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        if self.json {
            print_json(&profiles)?;
        } else {
            for profile in profiles {
                println!("{}", profile.name);
            }
        }

        Ok(())
    }

    fn profile_test(&self, args: ProfileTestArgs) -> Result<()> {
        let client = self.client_for_profile(&args.name)?;
        let domains = client.list_domains()?;
        if self.json {
            print_json(&domains)?;
        } else {
            for domain in domains.data {
                let sending = domain
                    .capabilities
                    .as_ref()
                    .and_then(|caps| caps.sending.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let receiving = domain
                    .capabilities
                    .as_ref()
                    .and_then(|caps| caps.receiving.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let status = domain.status.unwrap_or_else(|| "unknown".to_string());
                println!(
                    "{} status={} sending={} receiving={}",
                    domain.name, status, sending, receiving
                );
            }
        }
        Ok(())
    }

    fn account_add(&self, args: AccountAddArgs) -> Result<()> {
        let email = normalize_email(&args.email);
        let domain = email
            .split('@')
            .nth(1)
            .ok_or_else(|| anyhow!("invalid email: {}", email))?;

        let client = self.client_for_profile(&args.profile)?;
        let domains = client.list_domains()?;
        let matched = domains
            .data
            .into_iter()
            .find(|item| item.name.eq_ignore_ascii_case(domain))
            .ok_or_else(|| {
                anyhow!(
                    "domain {} is not present in profile {}",
                    domain,
                    args.profile
                )
            })?;

        let sending = matched
            .capabilities
            .as_ref()
            .and_then(|caps| caps.sending.clone())
            .unwrap_or_else(|| "unknown".to_string());
        if sending != "enabled" {
            bail!(
                "domain {} is not send-enabled in profile {}",
                domain,
                args.profile
            );
        }

        let tx = self.conn.unchecked_transaction()?;
        let has_default = tx
            .query_row(
                "SELECT 1 FROM accounts WHERE is_default = 1 LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        let existing_default = tx
            .query_row(
                "SELECT is_default FROM accounts WHERE email = ?1",
                params![email.clone()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            == 1;
        let is_default = if args.default {
            true
        } else if existing_default {
            true
        } else {
            !has_default
        };
        if is_default {
            tx.execute("UPDATE accounts SET is_default = 0", [])?;
        }

        tx.execute(
            "
            INSERT INTO accounts (email, profile_name, display_name, signature, is_default, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
            ON CONFLICT(email) DO UPDATE SET
                profile_name = excluded.profile_name,
                display_name = excluded.display_name,
                signature = excluded.signature,
                is_default = excluded.is_default,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                email,
                args.profile,
                args.name,
                args.signature,
                if is_default { 1 } else { 0 }
            ],
        )?;
        tx.commit()?;

        let account = self.get_account(&email)?;
        if self.json {
            print_json(&account)?;
        } else {
            println!(
                "saved account {} on profile {}{}",
                account.email,
                account.profile_name,
                if is_default { " (default)" } else { "" }
            );
        }

        Ok(())
    }

    fn account_list(&self) -> Result<()> {
        let accounts = self.list_accounts()?;
        if self.json {
            print_json(&accounts)?;
        } else {
            for account in accounts {
                let marker = if account.is_default { " *" } else { "" };
                println!("{} [{}]{}", account.email, account.profile_name, marker);
            }
        }
        Ok(())
    }

    fn account_use(&self, args: AccountUseArgs) -> Result<()> {
        let email = normalize_email(&args.email);
        self.get_account(&email)?;
        self.conn
            .execute("UPDATE accounts SET is_default = 0", [])?;
        self.conn.execute(
            "UPDATE accounts SET is_default = 1, updated_at = CURRENT_TIMESTAMP WHERE email = ?1",
            params![email],
        )?;
        if self.json {
            print_json(&json!({"default_account": email}))?;
        } else {
            println!("default account {}", email);
        }
        Ok(())
    }

    fn signature_set(&self, args: SignatureSetArgs) -> Result<()> {
        let account = normalize_email(&args.account);
        self.conn.execute(
            "UPDATE accounts SET signature = ?1, updated_at = CURRENT_TIMESTAMP WHERE email = ?2",
            params![args.text, account],
        )?;
        let updated = self.get_account(&account)?;
        if self.json {
            print_json(&updated)?;
        } else {
            println!("updated signature for {}", updated.email);
        }
        Ok(())
    }

    fn signature_show(&self, args: SignatureShowArgs) -> Result<()> {
        let account = self.get_account(&normalize_email(&args.account))?;
        if self.json {
            print_json(&json!({
                "account": account.email,
                "signature": account.signature
            }))?;
        } else {
            println!("{}", account.signature);
        }
        Ok(())
    }

    fn send(&self, args: SendArgs) -> Result<()> {
        let compose = self.resolve_compose(args.compose)?;
        let message = self.send_compose(compose, None)?;
        if self.json {
            print_json(&message)?;
        } else {
            println!(
                "sent message {} from {} to {}",
                message.id,
                message.account_email,
                message.to.join(", ")
            );
        }
        Ok(())
    }

    fn reply(&self, args: ReplyArgs) -> Result<()> {
        let target = self.get_message(args.message_id)?;
        let account = match args.account {
            Some(account) => self.get_account(&normalize_email(&account))?,
            None => self.get_account(&target.account_email)?,
        };
        ensure_reply_account_matches(&target, &account)?;
        let recipients = reply_recipients(&target)?;
        let subject = reply_subject(&target.subject);
        let compose = ResolvedCompose {
            account,
            to: recipients,
            cc: Vec::new(),
            bcc: Vec::new(),
            subject,
            text: read_optional_content(args.text, args.text_file)?,
            html: read_optional_content(args.html, args.html_file)?,
            attachments: args.attachments,
        };
        let headers = reply_headers_for_message(&target);
        let message = self.send_compose(compose, Some((target.id, headers)))?;
        if self.json {
            print_json(&message)?;
        } else {
            println!("replied with message {}", message.id);
        }
        Ok(())
    }

    fn draft_create(&self, args: DraftCreateArgs) -> Result<()> {
        let compose = self.resolve_compose(args.compose)?;
        let id = Uuid::new_v4().to_string();
        if let Some(message_id) = args.reply_to_message_id {
            let target = self.get_message(message_id)?;
            ensure_reply_account_matches(&target, &compose.account)?;
        }
        let attachment_paths = snapshot_draft_attachments(
            self.db_path.parent().unwrap_or(Path::new(".")),
            &id,
            &compose.attachments,
        )?;
        self.conn.execute(
            "
            INSERT INTO drafts (
                id, account_email, to_json, cc_json, bcc_json, subject,
                text_body, html_body, reply_to_message_id, attachment_paths_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ",
            params![
                id,
                compose.account.email,
                to_json(&compose.to)?,
                to_json(&compose.cc)?,
                to_json(&compose.bcc)?,
                compose.subject,
                compose.text,
                compose.html,
                args.reply_to_message_id,
                to_json(&attachment_paths)?,
            ],
        )?;
        let draft = self.get_draft(&id)?;
        if self.json {
            print_json(&draft)?;
        } else {
            println!("saved draft {}", draft.id);
        }
        Ok(())
    }

    fn draft_list(&self, args: DraftListArgs) -> Result<()> {
        let drafts = if let Some(account) = args.account {
            let account = normalize_email(&account);
            self.list_drafts_for_account(&account)?
        } else {
            self.list_all_drafts()?
        };
        if self.json {
            print_json(&drafts)?;
        } else {
            for draft in drafts {
                println!("{} {} {}", draft.id, draft.account_email, draft.subject);
            }
        }
        Ok(())
    }

    fn draft_show(&self, args: DraftShowArgs) -> Result<()> {
        let draft = self.get_draft(&args.id)?;
        if self.json {
            print_json(&draft)?;
        } else {
            println!("draft {}", draft.id);
            println!("account: {}", draft.account_email);
            println!("to: {}", draft.to.join(", "));
            println!("subject: {}", draft.subject);
            if let Some(text) = draft.text_body {
                println!();
                println!("{}", text);
            }
        }
        Ok(())
    }

    fn draft_send(&self, args: DraftSendArgs) -> Result<()> {
        let draft = self.get_draft(&args.id)?;
        let account = self.get_account(&draft.account_email)?;
        let reply_context = if let Some(message_id) = draft.reply_to_message_id {
            let target = self.get_message(message_id)?;
            ensure_reply_account_matches(&target, &account)?;
            Some((target.id, reply_headers_for_message(&target)))
        } else {
            None
        };
        let compose = ResolvedCompose {
            account,
            to: draft.to.clone(),
            cc: draft.cc.clone(),
            bcc: draft.bcc.clone(),
            subject: draft.subject.clone(),
            text: draft.text_body.clone(),
            html: draft.html_body.clone(),
            attachments: draft
                .attachment_paths
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>(),
        };
        let message = self.send_compose(compose, reply_context)?;
        self.conn
            .execute("DELETE FROM drafts WHERE id = ?1", params![draft.id])?;
        remove_draft_attachment_snapshot(
            self.db_path.parent().unwrap_or(Path::new(".")),
            &draft.id,
        )?;
        if self.json {
            print_json(&message)?;
        } else {
            println!("sent draft as message {}", message.id);
        }
        Ok(())
    }

    fn sync(&self, args: SyncArgs) -> Result<()> {
        let accounts = if let Some(account) = args.account {
            vec![self.get_account(&normalize_email(&account))?]
        } else {
            self.list_accounts()?
        };
        if accounts.is_empty() {
            bail!("no accounts configured");
        }

        let unique_profiles = accounts
            .iter()
            .map(|account| account.profile_name.clone())
            .collect::<std::collections::BTreeSet<_>>();

        let mut summary = SyncSummary {
            profiles: unique_profiles.len(),
            sent_messages: 0,
            received_messages: 0,
        };

        for account in accounts {
            let client = self.client_for_profile(&account.profile_name)?;
            summary.sent_messages += self.sync_sent_account(&client, &account, args.limit)?;
            summary.received_messages +=
                self.sync_received_account(&client, &account, args.limit)?;
        }

        if self.json {
            print_json(&summary)?;
        } else {
            println!(
                "synced profiles={} sent={} received={}",
                summary.profiles, summary.sent_messages, summary.received_messages
            );
        }
        Ok(())
    }

    fn inbox_list(&self, args: InboxListArgs) -> Result<()> {
        let messages = self.list_messages(args.account.as_deref(), args.limit, args.unread)?;
        if self.json {
            print_json(&messages)?;
        } else {
            for message in messages {
                let read_flag = if message.is_read { " " } else { "*" };
                println!(
                    "{}{} [{}] {} -> {} | {}",
                    message.id,
                    read_flag,
                    message.direction,
                    message.account_email,
                    compact_targets(&message.to),
                    message.subject
                );
            }
        }
        Ok(())
    }

    fn inbox_read(&self, args: InboxReadArgs) -> Result<()> {
        if args.mark_read {
            self.conn.execute(
                "UPDATE messages SET is_read = 1 WHERE id = ?1",
                params![args.id],
            )?;
        }
        let message = self.get_message(args.id)?;
        if self.json {
            print_json(&message)?;
        } else {
            println!("id: {}", message.id);
            println!("account: {}", message.account_email);
            println!("direction: {}", message.direction);
            println!("from: {}", message.from_addr);
            println!("to: {}", message.to.join(", "));
            println!("subject: {}", message.subject);
            if let Some(rfc) = message.rfc_message_id.as_deref() {
                println!("message-id: {}", rfc);
            }
            println!();
            if let Some(text) = message.text_body.as_deref() {
                println!("{}", text);
            } else if let Some(html) = message.html_body.as_deref() {
                println!("{}", html);
            }
        }
        Ok(())
    }

    fn attachments_list(&self, args: AttachmentListArgs) -> Result<()> {
        let message = self.get_message(args.message_id)?;
        if message.direction == "received" {
            let account = self.get_account(&message.account_email)?;
            let client = self.client_for_profile(&account.profile_name)?;
            let attachments = client.list_received_attachments(&message.remote_id)?;
            self.store_received_attachments(message.id, &attachments)?;
        }
        let rows = self.list_attachments(args.message_id)?;
        if self.json {
            print_json(&rows)?;
        } else {
            for row in rows {
                let remote = row
                    .remote_attachment_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                let name = row.filename.unwrap_or_else(|| "attachment".to_string());
                println!("{} {}", remote, name);
            }
        }
        Ok(())
    }

    fn attachments_get(&self, args: AttachmentGetArgs) -> Result<()> {
        let message = self.get_message(args.message_id)?;
        if message.direction != "received" {
            bail!("attachment download is only supported for received messages");
        }
        let account = self.get_account(&message.account_email)?;
        let client = self.client_for_profile(&account.profile_name)?;
        let attachments = client.list_received_attachments(&message.remote_id)?;
        self.store_received_attachments(message.id, &attachments)?;
        let attachment = self
            .find_attachment(args.message_id, &args.attachment_id)?
            .ok_or_else(|| anyhow!("attachment {} not found", args.attachment_id))?;
        let download_url = attachment
            .download_url
            .clone()
            .ok_or_else(|| anyhow!("attachment {} has no download url", args.attachment_id))?;
        let output_dir = args.output.unwrap_or_else(|| {
            self.db_path
                .parent()
                .unwrap_or(Path::new("."))
                .join("downloads")
        });
        fs::create_dir_all(&output_dir)?;
        let preferred_filename = attachment
            .filename
            .clone()
            .unwrap_or_else(|| format!("attachment-{}", args.attachment_id));
        let bytes = client.download_attachment(&download_url)?;
        let output_path = write_file_safely(&output_dir, &preferred_filename, &bytes)?;
        self.conn.execute(
            "UPDATE attachments SET local_path = ?1 WHERE id = ?2",
            params![output_path.display().to_string(), attachment.id],
        )?;
        if self.json {
            print_json(&json!({
                "message_id": args.message_id,
                "attachment_id": args.attachment_id,
                "path": output_path.display().to_string(),
            }))?;
        } else {
            println!("{}", output_path.display());
        }
        Ok(())
    }

    fn client_for_profile(&self, name: &str) -> Result<ResendClient> {
        let api_key: String = self
            .conn
            .query_row(
                "SELECT api_key FROM profiles WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .with_context(|| format!("profile {} not found", name))?;
        ResendClient::new(api_key)
    }

    fn list_accounts(&self) -> Result<Vec<AccountRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT email, profile_name, display_name, signature, is_default, created_at, updated_at
            FROM accounts
            ORDER BY is_default DESC, email
            ",
        )?;
        let rows = stmt.query_map([], map_account)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn get_account(&self, email: &str) -> Result<AccountRecord> {
        self.conn
            .query_row(
                "
                SELECT email, profile_name, display_name, signature, is_default, created_at, updated_at
                FROM accounts
                WHERE email = ?1
                ",
                params![email],
                map_account,
            )
            .with_context(|| format!("account {} not found", email))
    }

    fn default_account(&self) -> Result<AccountRecord> {
        self.conn
            .query_row(
                "
                SELECT email, profile_name, display_name, signature, is_default, created_at, updated_at
                FROM accounts
                WHERE is_default = 1
                LIMIT 1
                ",
                [],
                map_account,
            )
            .context("no default account configured")
    }

    fn resolve_compose(&self, compose: ComposeArgs) -> Result<ResolvedCompose> {
        let account = match compose.account {
            Some(account) => self.get_account(&normalize_email(&account))?,
            None => self.default_account()?,
        };
        let text = read_optional_content(compose.text, compose.text_file)?;
        let html = read_optional_content(compose.html, compose.html_file)?;
        if text.is_none() && html.is_none() {
            bail!("one of --text/--text-file or --html/--html-file is required");
        }
        Ok(ResolvedCompose {
            account,
            to: normalize_emails(&compose.to),
            cc: normalize_emails(&compose.cc),
            bcc: normalize_emails(&compose.bcc),
            subject: compose.subject,
            text,
            html,
            attachments: compose.attachments,
        })
    }

    fn send_compose(
        &self,
        compose: ResolvedCompose,
        reply_context: Option<(i64, ReplyHeaders)>,
    ) -> Result<MessageRecord> {
        let client = self.client_for_profile(&compose.account.profile_name)?;
        let mut text = compose.text.clone();
        let mut html = compose.html.clone();
        if !compose.account.signature.trim().is_empty() {
            if text.is_some() {
                text = Some(append_signature_text(
                    text.as_deref(),
                    &compose.account.signature,
                ));
            }
            if html.is_some() {
                html = Some(append_signature_html(
                    html.as_deref(),
                    &compose.account.signature,
                ));
            }
        }

        let headers = reply_context.as_ref().and_then(|(_, reply)| {
            if reply.in_reply_to.is_none() && reply.references.is_empty() {
                None
            } else {
                let mut headers = HashMap::new();
                if let Some(in_reply_to) = reply.in_reply_to.as_deref() {
                    headers.insert("In-Reply-To".to_string(), in_reply_to.to_string());
                }
                if !reply.references.is_empty() {
                    headers.insert("References".to_string(), reply.references.join(" "));
                }
                Some(headers)
            }
        });

        let request = SendEmailRequest {
            from: format_sender(
                compose.account.display_name.as_deref(),
                &compose.account.email,
            ),
            to: compose.to.clone(),
            cc: compose.cc.clone(),
            bcc: compose.bcc.clone(),
            subject: compose.subject.clone(),
            text,
            html,
            headers,
            attachments: build_send_attachments(&compose.attachments)?,
        };
        let idempotency_key = build_idempotency_key(&request)?;

        let response = client.send_email(&request, &idempotency_key)?;
        let detail = fetch_sent_detail(&client, &response.id).unwrap_or_else(|| SentEmail {
            id: response.id.clone(),
            from: Some(request.from.clone()),
            to: request.to.clone(),
            cc: request.cc.clone(),
            bcc: request.bcc.clone(),
            reply_to: Vec::new(),
            subject: Some(request.subject.clone()),
            created_at: Some(now_timestamp()),
            last_event: Some("sent".to_string()),
            html: request.html.clone(),
            text: request.text.clone(),
        });
        let reply_headers = reply_context.map(|(_, reply)| reply);
        self.store_sent_message(&compose.account, detail, reply_headers)
    }

    fn store_sent_message(
        &self,
        account: &AccountRecord,
        email: SentEmail,
        reply_headers: Option<ReplyHeaders>,
    ) -> Result<MessageRecord> {
        let raw_json = serde_json::to_string(&email)?;
        let created_at = normalize_timestamp(email.created_at.as_deref());
        let references = reply_headers
            .as_ref()
            .map(|reply| reply.references.clone())
            .unwrap_or_default();
        let in_reply_to = reply_headers.and_then(|reply| reply.in_reply_to);
        self.upsert_message(MessageUpsert {
            remote_id: email.id,
            direction: "sent".to_string(),
            account_email: account.email.clone(),
            from_addr: email.from.unwrap_or_else(|| account.email.clone()),
            to: email.to,
            cc: email.cc,
            bcc: email.bcc,
            reply_to: email.reply_to,
            subject: email.subject.unwrap_or_default(),
            text_body: email.text,
            html_body: email.html,
            rfc_message_id: None,
            in_reply_to,
            references,
            last_event: email.last_event,
            is_read: true,
            created_at,
            raw_json,
        })
    }

    fn store_received_message(&self, account: &AccountRecord, email: ReceivedEmail) -> Result<i64> {
        let raw_json = serde_json::to_string(&email)?;
        let created_at = normalize_timestamp(email.created_at.as_deref());
        let headers = email.headers.clone().unwrap_or_default();
        let references = header_references(&headers);
        let in_reply_to = header_string(&headers, "in-reply-to");
        let record = self.upsert_message(MessageUpsert {
            remote_id: email.id.clone(),
            direction: "received".to_string(),
            account_email: account.email.clone(),
            from_addr: email.from.unwrap_or_default(),
            to: email.to.clone(),
            cc: email.cc.clone(),
            bcc: email.bcc.clone(),
            reply_to: email.reply_to.clone(),
            subject: email.subject.unwrap_or_default(),
            text_body: email.text.clone(),
            html_body: email.html.clone(),
            rfc_message_id: email.message_id.clone(),
            in_reply_to,
            references,
            last_event: Some("received".to_string()),
            is_read: false,
            created_at,
            raw_json,
        })?;
        Ok(record.id)
    }

    fn store_received_attachments(
        &self,
        message_id: i64,
        attachments: &[ReceivedAttachment],
    ) -> Result<()> {
        for attachment in attachments {
            self.conn.execute(
                "
                INSERT INTO attachments (
                    message_id, remote_attachment_id, filename, content_type, size, download_url, raw_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(message_id, remote_attachment_id, filename) DO UPDATE SET
                    content_type = excluded.content_type,
                    size = excluded.size,
                    download_url = excluded.download_url,
                    raw_json = excluded.raw_json
                ",
                params![
                    message_id,
                    attachment.id,
                    attachment.filename,
                    attachment.content_type,
                    attachment.size,
                    attachment.download_url,
                    serde_json::to_string(attachment)?,
                ],
            )?;
        }
        Ok(())
    }

    fn upsert_message(&self, payload: MessageUpsert) -> Result<MessageRecord> {
        let existing_id: Option<i64> = self
            .conn
            .query_row(
                "
                SELECT id
                FROM messages
                WHERE remote_id = ?1 AND direction = ?2 AND account_email = ?3
                ",
                params![payload.remote_id, payload.direction, payload.account_email],
                |row| row.get(0),
            )
            .optional()?;

        match existing_id {
            Some(id) => {
                self.conn.execute(
                    "
                    UPDATE messages
                    SET from_addr = ?1,
                        to_json = ?2,
                        cc_json = ?3,
                        bcc_json = ?4,
                        reply_to_json = ?5,
                        subject = ?6,
                        text_body = ?7,
                        html_body = ?8,
                        rfc_message_id = COALESCE(?9, rfc_message_id),
                        in_reply_to = COALESCE(?10, in_reply_to),
                        references_json = ?11,
                        last_event = ?12,
                        created_at = ?13,
                        synced_at = CURRENT_TIMESTAMP,
                        raw_json = ?14
                    WHERE id = ?15
                    ",
                    params![
                        payload.from_addr,
                        to_json(&payload.to)?,
                        to_json(&payload.cc)?,
                        to_json(&payload.bcc)?,
                        to_json(&payload.reply_to)?,
                        payload.subject,
                        payload.text_body,
                        payload.html_body,
                        payload.rfc_message_id,
                        payload.in_reply_to,
                        to_json(&payload.references)?,
                        payload.last_event,
                        payload.created_at,
                        payload.raw_json,
                        id,
                    ],
                )?;
                self.get_message(id)
            }
            None => {
                self.conn.execute(
                    "
                    INSERT INTO messages (
                        remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                        reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                        references_json, last_event, is_read, created_at, raw_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                    ",
                    params![
                        payload.remote_id,
                        payload.direction,
                        payload.account_email,
                        payload.from_addr,
                        to_json(&payload.to)?,
                        to_json(&payload.cc)?,
                        to_json(&payload.bcc)?,
                        to_json(&payload.reply_to)?,
                        payload.subject,
                        payload.text_body,
                        payload.html_body,
                        payload.rfc_message_id,
                        payload.in_reply_to,
                        to_json(&payload.references)?,
                        payload.last_event,
                        if payload.is_read { 1 } else { 0 },
                        payload.created_at,
                        payload.raw_json,
                    ],
                )?;
                let id = self.conn.last_insert_rowid();
                self.get_message(id)
            }
        }
    }

    fn list_messages(
        &self,
        account: Option<&str>,
        limit: usize,
        unread_only: bool,
    ) -> Result<Vec<MessageRecord>> {
        let sql = match (account, unread_only) {
            (Some(_), true) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE account_email = ?1 AND is_read = 0
                ORDER BY created_at DESC
                LIMIT ?2
                "
            }
            (Some(_), false) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE account_email = ?1
                ORDER BY created_at DESC
                LIMIT ?2
                "
            }
            (None, true) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE is_read = 0
                ORDER BY created_at DESC
                LIMIT ?1
                "
            }
            (None, false) => {
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                ORDER BY created_at DESC
                LIMIT ?1
                "
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match (account, unread_only) {
            (Some(account), _) => {
                stmt.query_map(params![normalize_email(account), limit as i64], map_message)?
            }
            (None, _) => stmt.query_map(params![limit as i64], map_message)?,
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn get_message(&self, id: i64) -> Result<MessageRecord> {
        self.conn
            .query_row(
                "
                SELECT id, remote_id, direction, account_email, from_addr, to_json, cc_json, bcc_json,
                       reply_to_json, subject, text_body, html_body, rfc_message_id, in_reply_to,
                       references_json, last_event, is_read, created_at, synced_at
                FROM messages
                WHERE id = ?1
                ",
                params![id],
                map_message,
            )
            .with_context(|| format!("message {} not found", id))
    }

    fn list_all_drafts(&self) -> Result<Vec<DraftRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, account_email, to_json, cc_json, bcc_json, subject, text_body, html_body,
                   reply_to_message_id, attachment_paths_json, created_at, updated_at
            FROM drafts
            ORDER BY updated_at DESC
            ",
        )?;
        let rows = stmt.query_map([], map_draft)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn list_drafts_for_account(&self, account: &str) -> Result<Vec<DraftRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, account_email, to_json, cc_json, bcc_json, subject, text_body, html_body,
                   reply_to_message_id, attachment_paths_json, created_at, updated_at
            FROM drafts
            WHERE account_email = ?1
            ORDER BY updated_at DESC
            ",
        )?;
        let rows = stmt.query_map(params![account], map_draft)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn get_draft(&self, id: &str) -> Result<DraftRecord> {
        self.conn
            .query_row(
                "
                SELECT id, account_email, to_json, cc_json, bcc_json, subject, text_body, html_body,
                       reply_to_message_id, attachment_paths_json, created_at, updated_at
                FROM drafts
                WHERE id = ?1
                ",
                params![id],
                map_draft,
            )
            .with_context(|| format!("draft {} not found", id))
    }

    fn list_attachments(&self, message_id: i64) -> Result<Vec<AttachmentRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, message_id, remote_attachment_id, filename, content_type, size, download_url, local_path
            FROM attachments
            WHERE message_id = ?1
            ORDER BY id
            ",
        )?;
        let rows = stmt.query_map(params![message_id], map_attachment)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn find_attachment(
        &self,
        message_id: i64,
        attachment_id: &str,
    ) -> Result<Option<AttachmentRecord>> {
        self.conn
            .query_row(
                "
                SELECT id, message_id, remote_attachment_id, filename, content_type, size, download_url, local_path
                FROM attachments
                WHERE message_id = ?1 AND remote_attachment_id = ?2
                ",
                params![message_id, attachment_id],
                map_attachment,
            )
            .optional()
            .map_err(Into::into)
    }

    fn get_sync_cursor(&self, account_email: &str, direction: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "
                SELECT cursor_id
                FROM sync_state
                WHERE account_email = ?1 AND direction = ?2
                ",
                params![account_email, direction],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    fn set_sync_cursor(&self, account_email: &str, direction: &str, cursor_id: &str) -> Result<()> {
        self.conn.execute(
            "
            INSERT INTO sync_state (account_email, direction, cursor_id, updated_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT(account_email, direction) DO UPDATE SET
                cursor_id = excluded.cursor_id,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![account_email, direction, cursor_id],
        )?;
        Ok(())
    }

    fn sync_sent_account(
        &self,
        client: &ResendClient,
        account: &AccountRecord,
        page_size: usize,
    ) -> Result<usize> {
        let cursor = self.get_sync_cursor(&account.email, "sent")?;
        let mut after = None;
        let mut newest_cursor = None;
        let mut total = 0usize;

        loop {
            let page = client.list_sent_emails_page(page_size, after.as_deref())?;
            if newest_cursor.is_none() {
                newest_cursor = page.data.first().map(|item| item.id.clone());
            }
            let mut stop = false;
            let mut last_id = None;

            for item in page.data {
                last_id = Some(item.id.clone());
                if cursor.as_deref() == Some(item.id.as_str()) {
                    stop = true;
                    break;
                }
                let from_email = item
                    .from
                    .as_deref()
                    .map(normalize_email)
                    .unwrap_or_default();
                if from_email == account.email {
                    let detail = client.get_sent_email(&item.id)?;
                    self.store_sent_message(account, detail, None)?;
                    total += 1;
                }
            }

            if stop || !page.has_more.unwrap_or(false) || last_id.is_none() {
                break;
            }
            after = last_id;
        }

        if let Some(cursor_id) = newest_cursor {
            self.set_sync_cursor(&account.email, "sent", &cursor_id)?;
        }

        Ok(total)
    }

    fn sync_received_account(
        &self,
        client: &ResendClient,
        account: &AccountRecord,
        page_size: usize,
    ) -> Result<usize> {
        let cursor = self.get_sync_cursor(&account.email, "received")?;
        let mut after = None;
        let mut newest_cursor = None;
        let mut total = 0usize;

        loop {
            let page = client.list_received_emails_page(page_size, after.as_deref())?;
            if newest_cursor.is_none() {
                newest_cursor = page.data.first().map(|item| item.id.clone());
            }
            let mut stop = false;
            let mut last_id = None;

            for item in page.data {
                last_id = Some(item.id.clone());
                if cursor.as_deref() == Some(item.id.as_str()) {
                    stop = true;
                    break;
                }
                let matches = matching_account_email(&item.to, &item.cc, &item.bcc, &account.email);
                if !matches {
                    continue;
                }
                let detail = client.get_received_email(&item.id)?;
                if !matching_account_email(&detail.to, &detail.cc, &detail.bcc, &account.email) {
                    continue;
                }
                let message_id = self.store_received_message(account, detail.clone())?;
                self.store_received_attachments(message_id, &detail.attachments)?;
                total += 1;
            }

            if stop || !page.has_more.unwrap_or(false) || last_id.is_none() {
                break;
            }
            after = last_id;
        }

        if let Some(cursor_id) = newest_cursor {
            self.set_sync_cursor(&account.email, "received", &cursor_id)?;
        }

        Ok(total)
    }
}

impl ResendClient {
    fn new(api_key: String) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent("email-cli/0.1.0")
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()
                .context("failed to build http client")?,
            api_key,
        })
    }

    fn list_domains(&self) -> Result<DomainList> {
        self.get_json("/domains", &[])
    }

    fn send_email(
        &self,
        payload: &SendEmailRequest,
        idempotency_key: &str,
    ) -> Result<SendEmailResponse> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Idempotency-Key",
            HeaderValue::from_str(idempotency_key).context("invalid idempotency key")?,
        );
        self.post_json("/emails", payload, Some(headers))
    }

    fn list_sent_emails_page(
        &self,
        limit: usize,
        after: Option<&str>,
    ) -> Result<ListResponse<SentEmail>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        self.get_json("/emails", &query)
    }

    fn get_sent_email(&self, id: &str) -> Result<SentEmail> {
        self.get_json(&format!("/emails/{}", id), &[])
    }

    fn list_received_emails_page(
        &self,
        limit: usize,
        after: Option<&str>,
    ) -> Result<ListResponse<ReceivedEmail>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        self.get_json("/emails/receiving", &query)
    }

    fn get_received_email(&self, id: &str) -> Result<ReceivedEmail> {
        self.get_json(&format!("/emails/receiving/{}", id), &[])
    }

    fn list_received_attachments(&self, email_id: &str) -> Result<Vec<ReceivedAttachment>> {
        let payload: ListResponse<ReceivedAttachment> =
            self.get_json(&format!("/emails/receiving/{}/attachments", email_id), &[])?;
        Ok(payload.data)
    }

    fn download_attachment(&self, url: &str) -> Result<Vec<u8>> {
        for attempt in 0..5 {
            let response = match self.client.get(url).send() {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).context("attachment download failed"),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_bytes(response);
        }
        bail!("attachment download kept rate limiting")
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        for attempt in 0..5 {
            let response = match self
                .client
                .get(format!("https://api.resend.com{}", path))
                .bearer_auth(&self.api_key)
                .query(query)
                .send()
            {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).with_context(|| format!("GET {} failed", path)),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_json(response);
        }
        bail!("Resend API kept rate limiting for {}", path)
    }

    fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
        headers: Option<HeaderMap>,
    ) -> Result<T> {
        for attempt in 0..5 {
            let mut request = self
                .client
                .post(format!("https://api.resend.com{}", path))
                .bearer_auth(&self.api_key)
                .json(body);
            if let Some(extra_headers) = headers.clone() {
                request = request.headers(extra_headers);
            }
            let response = match request.send() {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).with_context(|| format!("POST {} failed", path)),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_json(response);
        }
        bail!("Resend API kept rate limiting for {}", path)
    }
}

#[derive(Clone)]
struct MessageUpsert {
    remote_id: String,
    direction: String,
    account_email: String,
    from_addr: String,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    reply_to: Vec<String>,
    subject: String,
    text_body: Option<String>,
    html_body: Option<String>,
    rfc_message_id: Option<String>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    last_event: Option<String>,
    is_read: bool,
    created_at: String,
    raw_json: String,
}

fn default_db_path() -> Result<PathBuf> {
    let base = data_local_dir().unwrap_or(std::env::current_dir()?);
    Ok(base.join("email-cli").join("email-cli.db"))
}

fn resolve_api_key(
    direct: Option<String>,
    env_var: Option<String>,
    file: Option<PathBuf>,
    env_name: &str,
) -> Result<String> {
    if let Some(key) = direct {
        let trimmed = cleanup_env_value(&key);
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    if let Some(name) = env_var {
        let value = std::env::var(&name)
            .with_context(|| format!("environment variable {} is not set", name))?;
        let cleaned = cleanup_env_value(&value);
        if cleaned.is_empty() {
            bail!("environment variable {} is empty", name);
        }
        return Ok(cleaned);
    }
    if let Some(path) = file {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        for line in content.lines() {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == env_name {
                    let cleaned = cleanup_env_value(value);
                    if cleaned.is_empty() {
                        bail!("{} in {} is empty", env_name, path.display());
                    }
                    return Ok(cleaned);
                }
            }
        }
        bail!("{} not found in {}", env_name, path.display());
    }
    bail!("provide one of --api-key, --api-key-env, or --api-key-file")
}

fn cleanup_env_value(value: &str) -> String {
    let mut value = value.trim().to_string();
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        value = value[1..value.len() - 1].to_string();
    }
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        value = value[1..value.len() - 1].to_string();
    }
    value.replace("\\n", "").trim().to_string()
}

fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<AccountRecord> {
    Ok(AccountRecord {
        email: row.get(0)?,
        profile_name: row.get(1)?,
        display_name: row.get(2)?,
        signature: row.get(3)?,
        is_default: row.get::<_, i64>(4)? == 1,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        id: row.get(0)?,
        remote_id: row.get(1)?,
        direction: row.get(2)?,
        account_email: row.get(3)?,
        from_addr: row.get(4)?,
        to: from_json(&row.get::<_, String>(5)?).unwrap_or_default(),
        cc: from_json(&row.get::<_, String>(6)?).unwrap_or_default(),
        bcc: from_json(&row.get::<_, String>(7)?).unwrap_or_default(),
        reply_to: from_json(&row.get::<_, String>(8)?).unwrap_or_default(),
        subject: row.get(9)?,
        text_body: row.get(10)?,
        html_body: row.get(11)?,
        rfc_message_id: row.get(12)?,
        in_reply_to: row.get(13)?,
        references: from_json(&row.get::<_, String>(14)?).unwrap_or_default(),
        last_event: row.get(15)?,
        is_read: row.get::<_, i64>(16)? == 1,
        created_at: row.get(17)?,
        synced_at: row.get(18)?,
    })
}

fn map_draft(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftRecord> {
    Ok(DraftRecord {
        id: row.get(0)?,
        account_email: row.get(1)?,
        to: from_json(&row.get::<_, String>(2)?).unwrap_or_default(),
        cc: from_json(&row.get::<_, String>(3)?).unwrap_or_default(),
        bcc: from_json(&row.get::<_, String>(4)?).unwrap_or_default(),
        subject: row.get(5)?,
        text_body: row.get(6)?,
        html_body: row.get(7)?,
        reply_to_message_id: row.get(8)?,
        attachment_paths: from_json(&row.get::<_, String>(9)?).unwrap_or_default(),
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn map_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRecord> {
    Ok(AttachmentRecord {
        id: row.get(0)?,
        message_id: row.get(1)?,
        remote_attachment_id: row.get(2)?,
        filename: row.get(3)?,
        content_type: row.get(4)?,
        size: row.get(5)?,
        download_url: row.get(6)?,
        local_path: row.get(7)?,
    })
}

fn read_optional_content(value: Option<String>, path: Option<PathBuf>) -> Result<Option<String>> {
    match (value, path) {
        (Some(text), None) => Ok(Some(text)),
        (None, Some(path)) => fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))
            .map(Some),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => bail!("use either inline content or a file, not both"),
    }
}

fn build_send_attachments(paths: &[PathBuf]) -> Result<Vec<SendAttachment>> {
    let mut attachments = Vec::new();
    for path in paths {
        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("invalid attachment path {}", path.display()))?
            .to_string();
        attachments.push(SendAttachment {
            filename,
            content: BASE64.encode(bytes),
        });
    }
    Ok(attachments)
}

fn normalize_email(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(start) = trimmed.rfind('<') {
        if let Some(end) = trimmed[start + 1..].find('>') {
            return trimmed[start + 1..start + 1 + end]
                .trim()
                .to_ascii_lowercase();
        }
    }
    trimmed.trim_matches('"').to_ascii_lowercase()
}

fn normalize_emails(values: &[String]) -> Vec<String> {
    values.iter().map(|value| normalize_email(value)).collect()
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).context("failed to serialize json")
}

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).context("failed to parse json")
}

fn format_sender(display_name: Option<&str>, email: &str) -> String {
    match display_name {
        Some(name) if !name.trim().is_empty() => format!("{} <{}>", name.trim(), email),
        _ => email.to_string(),
    }
}

fn append_signature_text(body: Option<&str>, signature: &str) -> String {
    let body = body.unwrap_or("").trim_end();
    let signature = signature.trim();
    if body.is_empty() {
        signature.to_string()
    } else {
        format!("{body}\n\n-- \n{signature}")
    }
}

fn append_signature_html(body: Option<&str>, signature: &str) -> String {
    let body = body.unwrap_or("").trim_end();
    let escaped_signature = escape_html(signature).replace('\n', "<br>");
    if body.is_empty() {
        escaped_signature
    } else {
        format!("{body}<br><br>-- <br>{escaped_signature}")
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn reply_subject(subject: &str) -> String {
    if subject.to_ascii_lowercase().starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {}", subject)
    }
}

fn header_string(headers: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    header_values(headers, key, false).into_iter().next()
}

fn header_references(headers: &BTreeMap<String, Value>) -> Vec<String> {
    header_values(headers, "references", true)
}

fn header_values(
    headers: &BTreeMap<String, Value>,
    key: &str,
    split_whitespace: bool,
) -> Vec<String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value_to_strings(value, split_whitespace))
        .unwrap_or_default()
}

fn value_to_strings(value: &Value, split_whitespace: bool) -> Vec<String> {
    match value {
        Value::String(text) => {
            if let Ok(values) = serde_json::from_str::<Vec<String>>(text) {
                return values;
            }
            if split_whitespace {
                text.split_whitespace()
                    .map(|item| item.to_string())
                    .collect()
            } else {
                vec![text.clone()]
            }
        }
        Value::Array(values) => values
            .iter()
            .flat_map(|item| value_to_strings(item, split_whitespace))
            .collect(),
        _ => Vec::new(),
    }
}

fn reply_headers_for_message(message: &MessageRecord) -> ReplyHeaders {
    let mut refs = message.references.clone();
    if let Some(message_id) = message.rfc_message_id.as_deref() {
        refs.push(message_id.to_string());
    }

    ReplyHeaders {
        in_reply_to: message.rfc_message_id.clone(),
        references: stable_dedup(refs),
    }
}

fn compact_targets(values: &[String]) -> String {
    if values.len() <= 2 {
        values.join(", ")
    } else {
        format!("{}, {} +{}", values[0], values[1], values.len() - 2)
    }
}

fn decode_json<T: DeserializeOwned>(response: Response) -> Result<T> {
    let status = response.status();
    let text = response.text().context("failed to read http response")?;
    if !status.is_success() {
        bail!("Resend API {}: {}", status, extract_error_message(&text));
    }
    serde_json::from_str(&text).context("failed to decode json response")
}

fn decode_bytes(response: Response) -> Result<Vec<u8>> {
    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        bail!(
            "download failed {}: {}",
            status,
            extract_error_message(&text)
        );
    }
    response
        .bytes()
        .map(|body| body.to_vec())
        .context("failed to read body")
}

fn extract_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(|message| message.as_str())
                .map(|message| message.to_string())
        })
        .unwrap_or_else(|| body.to_string())
}

fn backoff(attempt: usize) -> Duration {
    let millis = 700_u64.saturating_mul((attempt as u64) + 1);
    Duration::from_millis(millis)
}

fn retry_delay(headers: &HeaderMap, attempt: usize) -> Duration {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| backoff(attempt))
}

fn should_retry_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

fn fetch_sent_detail(client: &ResendClient, id: &str) -> Option<SentEmail> {
    for attempt in 0..3 {
        if let Ok(detail) = client.get_sent_email(id) {
            return Some(detail);
        }
        sleep(Duration::from_millis(300 * ((attempt as u64) + 1)));
    }
    None
}

fn now_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn normalize_timestamp(value: Option<&str>) -> String {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return now_timestamp();
    };

    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true);
    }

    if !value.contains('T') {
        let mut candidate = value.to_string();
        if has_short_numeric_offset(value) {
            candidate.push_str(":00");
        }
        if let Ok(parsed) = DateTime::parse_from_str(&candidate, "%Y-%m-%d %H:%M:%S%.f%:z") {
            return parsed
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true);
        }
    }

    now_timestamp()
}

fn has_short_numeric_offset(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    let sign = bytes[bytes.len() - 3];
    (sign == b'+' || sign == b'-')
        && bytes[bytes.len() - 2].is_ascii_digit()
        && bytes[bytes.len() - 1].is_ascii_digit()
}

fn stable_dedup(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

fn matching_account_email(
    to: &[String],
    cc: &[String],
    bcc: &[String],
    account_email: &str,
) -> bool {
    let account_email = normalize_email(account_email);
    to.iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .any(|value| normalize_email(value) == account_email)
}

fn ensure_reply_account_matches(message: &MessageRecord, account: &AccountRecord) -> Result<()> {
    if message.account_email != account.email {
        bail!(
            "message {} belongs to {}, not {}",
            message.id,
            message.account_email,
            account.email
        );
    }
    Ok(())
}

fn reply_recipients(message: &MessageRecord) -> Result<Vec<String>> {
    if message.direction != "received" {
        bail!(
            "replying to sent messages is not supported until sent message-id storage is implemented"
        );
    }
    let recipients = if !message.reply_to.is_empty() {
        normalize_emails(&message.reply_to)
    } else {
        vec![normalize_email(&message.from_addr)]
    };
    if recipients.is_empty() {
        bail!("message {} has no reply recipient", message.id);
    }
    Ok(recipients)
}

fn sanitize_filename(name: &str, fallback: &str) -> String {
    let basename = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    let sanitized = basename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .to_string();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn write_file_safely(dir: &Path, preferred_name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let safe_name = sanitize_filename(preferred_name, "attachment.bin");
    let stem = Path::new(&safe_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let ext = Path::new(&safe_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value))
        .unwrap_or_default();

    for index in 0..1000 {
        let candidate_name = if index == 0 {
            safe_name.clone()
        } else {
            format!("{stem}-{index}{ext}")
        };
        let candidate = dir.join(candidate_name);
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to create {}", candidate.display()));
            }
        };
        file.write_all(bytes)
            .with_context(|| format!("failed to write {}", candidate.display()))?;
        return Ok(candidate);
    }

    bail!("failed to allocate a safe filename for {}", safe_name)
}

fn draft_attachment_root(base_dir: &Path) -> PathBuf {
    base_dir.join("draft-attachments")
}

fn snapshot_draft_attachments(
    base_dir: &Path,
    draft_id: &str,
    attachments: &[PathBuf],
) -> Result<Vec<String>> {
    let snapshot_dir = draft_attachment_root(base_dir).join(draft_id);
    fs::create_dir_all(&snapshot_dir)?;
    let mut stored = Vec::new();
    for attachment in attachments {
        let bytes = fs::read(attachment)
            .with_context(|| format!("failed to read {}", attachment.display()))?;
        let preferred = attachment
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("attachment.bin");
        let path = write_file_safely(&snapshot_dir, preferred, &bytes)?;
        stored.push(path.display().to_string());
    }
    Ok(stored)
}

fn remove_draft_attachment_snapshot(base_dir: &Path, draft_id: &str) -> Result<()> {
    let snapshot_dir = draft_attachment_root(base_dir).join(draft_id);
    if snapshot_dir.exists() {
        fs::remove_dir_all(&snapshot_dir)
            .with_context(|| format!("failed to remove {}", snapshot_dir.display()))?;
    }
    Ok(())
}

fn build_idempotency_key(request: &SendEmailRequest) -> Result<String> {
    let payload = serde_json::to_vec(request).context("failed to serialize send request")?;
    let digest = Sha256::digest(payload);
    Ok(format!("email-cli-{:x}", digest))
}

fn deserialize_string_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|item| item.as_str().map(|value| value.to_string()))
            .collect()),
        Value::String(value) => Ok(vec![value]),
        Value::Null => Ok(Vec::new()),
        _ => Err(serde::de::Error::custom("expected string array or null")),
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
