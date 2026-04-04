use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "email-cli",
    version,
    about = "Agent-friendly email CLI for Resend"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
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
    Forward(ForwardArgs),
    /// Run as a menu bar daemon with notifications
    Daemon(DaemonArgs),
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
    /// Manage Resend domains
    Domain {
        #[command(subcommand)]
        command: DomainCommand,
    },
    /// Manage audiences
    Audience {
        #[command(subcommand)]
        command: AudienceCommand,
    },
    /// Manage contacts within an audience
    Contact {
        #[command(subcommand)]
        command: ContactCommand,
    },
    /// Send batch emails
    Batch {
        #[command(subcommand)]
        command: BatchCommand,
    },
    /// Manage Resend API keys
    ApiKey {
        #[command(subcommand)]
        command: ApiKeyCommand,
    },
    /// Manage the durable send outbox
    Outbox {
        #[command(subcommand)]
        command: OutboxCommand,
    },
    /// Webhook event listener
    Webhook {
        #[command(subcommand)]
        command: WebhookCommand,
    },
    /// View delivery events
    Events {
        #[command(subcommand)]
        command: EventsCommand,
    },
    /// Self-update from GitHub Releases
    Update {
        /// Check only, don't install
        #[arg(long)]
        check: bool,
    },
    /// Machine-readable capability manifest
    AgentInfo,
    /// Install skill file to agent platforms
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Generate shell completions
    Completions {
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    Add(ProfileAddArgs),
    #[command(visible_alias = "ls")]
    List,
    Test(ProfileTestArgs),
}

#[derive(Args)]
pub struct ProfileAddArgs {
    pub name: String,
    #[arg(long)]
    pub api_key: Option<String>,
    #[arg(long)]
    pub api_key_env: Option<String>,
    #[arg(long)]
    pub api_key_file: Option<PathBuf>,
    #[arg(long, default_value = "RESEND_API_KEY")]
    pub api_key_name: String,
}

#[derive(Args)]
pub struct ProfileTestArgs {
    pub name: String,
}

#[derive(Subcommand)]
pub enum AccountCommand {
    Add(AccountAddArgs),
    #[command(visible_alias = "ls")]
    List,
    Use(AccountUseArgs),
}

#[derive(Args)]
pub struct AccountAddArgs {
    pub email: String,
    #[arg(long)]
    pub profile: String,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub signature: Option<String>,
    #[arg(long)]
    pub default: bool,
}

#[derive(Args)]
pub struct AccountUseArgs {
    pub email: String,
}

#[derive(Subcommand)]
pub enum SignatureCommand {
    Set(SignatureSetArgs),
    Show(SignatureShowArgs),
}

#[derive(Args)]
pub struct SignatureSetArgs {
    pub account: String,
    #[arg(long)]
    pub text: String,
}

#[derive(Args)]
pub struct SignatureShowArgs {
    pub account: String,
}

#[derive(Args, Clone)]
pub struct ComposeArgs {
    #[arg(long, visible_alias = "from")]
    pub account: Option<String>,
    #[arg(long, required_unless_present = "reply_to_msg")]
    pub to: Vec<String>,
    #[arg(long)]
    pub cc: Vec<String>,
    #[arg(long)]
    pub bcc: Vec<String>,
    #[arg(long, default_value = "")]
    pub subject: String,
    /// Thread this email as a reply to a local message ID
    #[arg(long)]
    pub reply_to_msg: Option<i64>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub text_file: Option<PathBuf>,
    #[arg(long)]
    pub html: Option<String>,
    #[arg(long)]
    pub html_file: Option<PathBuf>,
    #[arg(long = "attach")]
    pub attachments: Vec<PathBuf>,
}

#[derive(Args)]
pub struct SendArgs {
    #[command(flatten)]
    pub compose: ComposeArgs,
}

#[derive(Args)]
pub struct ReplyArgs {
    pub message_id: i64,
    #[arg(long, visible_alias = "from")]
    pub account: Option<String>,
    /// Reply to all recipients (preserves CC)
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub text_file: Option<PathBuf>,
    #[arg(long)]
    pub html: Option<String>,
    #[arg(long)]
    pub html_file: Option<PathBuf>,
    #[arg(long = "attach")]
    pub attachments: Vec<PathBuf>,
}

#[derive(Args)]
pub struct DaemonArgs {
    /// Account to monitor (default: all accounts)
    #[arg(long)]
    pub account: Option<String>,
    /// Poll interval in seconds
    #[arg(long, default_value = "60")]
    pub interval: u64,
}

#[derive(Args)]
pub struct ForwardArgs {
    pub message_id: i64,
    #[arg(long, visible_alias = "from")]
    pub account: Option<String>,
    #[arg(long, required = true)]
    pub to: Vec<String>,
    #[arg(long)]
    pub cc: Vec<String>,
    #[arg(long)]
    pub bcc: Vec<String>,
    /// Optional preamble text before the forwarded content
    #[arg(long)]
    pub text: Option<String>,
}

#[derive(Subcommand)]
pub enum DraftCommand {
    #[command(visible_alias = "new")]
    Create(DraftCreateArgs),
    #[command(visible_alias = "ls")]
    List(DraftListArgs),
    Show(DraftShowArgs),
    Send(DraftSendArgs),
    Edit(DraftEditArgs),
    #[command(visible_alias = "rm")]
    Delete(DraftDeleteArgs),
}

#[derive(Args)]
pub struct DraftEditArgs {
    pub id: String,
    #[arg(long)]
    pub subject: Option<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub html: Option<String>,
    #[arg(long)]
    pub to: Option<Vec<String>>,
    #[arg(long)]
    pub cc: Option<Vec<String>>,
    #[arg(long)]
    pub bcc: Option<Vec<String>>,
}

#[derive(Args)]
pub struct DraftDeleteArgs {
    pub id: String,
}

#[derive(Args)]
pub struct DraftCreateArgs {
    #[command(flatten)]
    pub compose: ComposeArgs,
    #[arg(long)]
    pub reply_to: Option<i64>,
}

#[derive(Args)]
pub struct DraftListArgs {
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Args)]
pub struct DraftShowArgs {
    pub id: String,
}

#[derive(Args)]
pub struct DraftSendArgs {
    pub id: String,
}

#[derive(Args)]
pub struct SyncArgs {
    #[arg(long)]
    pub account: Option<String>,
    #[arg(long, default_value = "25")]
    pub limit: usize,
    /// Watch for new messages continuously
    #[arg(long)]
    pub watch: bool,
    /// Poll interval in seconds (requires --watch)
    #[arg(long, default_value = "60")]
    pub interval: Option<u64>,
    /// Send desktop notifications for new messages
    #[arg(long)]
    pub notify: bool,
}

#[derive(Subcommand)]
pub enum InboxCommand {
    #[command(visible_alias = "ls")]
    List(InboxListArgs),
    /// Sync messages from Resend (shortcut for top-level sync)
    Sync(InboxSyncArgs),
    Read(InboxReadArgs),
    #[command(visible_alias = "rm")]
    Delete(InboxDeleteArgs),
    Archive(InboxArchiveArgs),
    Search(InboxSearchArgs),
    Purge(InboxPurgeArgs),
}

#[derive(Args)]
pub struct InboxSyncArgs {
    #[arg(long)]
    pub account: Option<String>,
    #[arg(long, default_value = "25")]
    pub limit: usize,
}

#[derive(Args)]
pub struct InboxListArgs {
    #[arg(long)]
    pub account: Option<String>,
    #[arg(long, default_value = "25")]
    pub limit: usize,
    #[arg(long)]
    pub unread: bool,
    #[arg(long)]
    pub archived: bool,
}

#[derive(Args)]
pub struct InboxReadArgs {
    pub id: i64,
    /// Mark message as read (default: true, use --no-mark-read to skip)
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    pub mark_read: bool,
    #[arg(long)]
    pub raw: bool,
}

#[derive(Args)]
pub struct InboxDeleteArgs {
    pub id: i64,
}

#[derive(Args)]
pub struct InboxArchiveArgs {
    pub id: i64,
}

#[derive(Args)]
pub struct InboxSearchArgs {
    pub query: String,
    #[arg(long)]
    pub account: Option<String>,
    #[arg(long, default_value = "25")]
    pub limit: usize,
}

#[derive(Args)]
pub struct InboxPurgeArgs {
    /// Delete messages older than this date (YYYY-MM-DD)
    #[arg(long)]
    pub before: String,
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Subcommand)]
pub enum AttachmentsCommand {
    #[command(visible_alias = "ls")]
    List(AttachmentListArgs),
    #[command(visible_alias = "show")]
    Get(AttachmentGetArgs),
}

#[derive(Args)]
pub struct AttachmentListArgs {
    pub message_id: i64,
}

#[derive(Args)]
pub struct AttachmentGetArgs {
    pub message_id: i64,
    pub attachment_id: String,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum SkillAction {
    /// Write skill file to all detected agent platforms
    Install,
    /// Check which platforms have the skill installed
    Status,
}

// ── Domain commands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DomainCommand {
    #[command(visible_alias = "ls")]
    List,
    #[command(visible_alias = "show")]
    Get(DomainGetArgs),
    #[command(visible_alias = "new")]
    Create(DomainCreateArgs),
    Verify(DomainVerifyArgs),
    #[command(visible_alias = "rm")]
    Delete(DomainDeleteArgs),
    Update(DomainUpdateArgs),
}

#[derive(Args)]
pub struct DomainGetArgs {
    pub id: String,
}

#[derive(Args)]
pub struct DomainCreateArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub region: Option<String>,
}

#[derive(Args)]
pub struct DomainVerifyArgs {
    pub id: String,
}

#[derive(Args)]
pub struct DomainDeleteArgs {
    pub id: String,
}

#[derive(Args)]
pub struct DomainUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub open_tracking: Option<bool>,
    #[arg(long)]
    pub click_tracking: Option<bool>,
}

// ── Audience commands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AudienceCommand {
    #[command(visible_alias = "ls")]
    List,
    #[command(visible_alias = "show")]
    Get(AudienceGetArgs),
    #[command(visible_alias = "new")]
    Create(AudienceCreateArgs),
    #[command(visible_alias = "rm")]
    Delete(AudienceDeleteArgs),
}

#[derive(Args)]
pub struct AudienceGetArgs {
    pub id: String,
}

#[derive(Args)]
pub struct AudienceCreateArgs {
    #[arg(long)]
    pub name: String,
}

#[derive(Args)]
pub struct AudienceDeleteArgs {
    pub id: String,
}

// ── Contact commands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum ContactCommand {
    #[command(visible_alias = "ls")]
    List(ContactListArgs),
    #[command(visible_alias = "show")]
    Get(ContactGetArgs),
    #[command(visible_alias = "new")]
    Create(ContactCreateArgs),
    Update(ContactUpdateArgs),
    #[command(visible_alias = "rm")]
    Delete(ContactDeleteArgs),
}

#[derive(Args)]
pub struct ContactListArgs {
    #[arg(long)]
    pub audience: String,
}

#[derive(Args)]
pub struct ContactGetArgs {
    #[arg(long)]
    pub audience: String,
    pub id: String,
}

#[derive(Args)]
pub struct ContactCreateArgs {
    #[arg(long)]
    pub audience: String,
    #[arg(long)]
    pub email: String,
    #[arg(long)]
    pub first_name: Option<String>,
    #[arg(long)]
    pub last_name: Option<String>,
    #[arg(long)]
    pub unsubscribed: Option<bool>,
}

#[derive(Args)]
pub struct ContactUpdateArgs {
    #[arg(long)]
    pub audience: String,
    pub id: String,
    #[arg(long)]
    pub first_name: Option<String>,
    #[arg(long)]
    pub last_name: Option<String>,
    #[arg(long)]
    pub unsubscribed: Option<bool>,
}

#[derive(Args)]
pub struct ContactDeleteArgs {
    #[arg(long)]
    pub audience: String,
    pub id: String,
}

// ── Batch commands ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum BatchCommand {
    Send(BatchSendArgs),
}

#[derive(Args)]
pub struct BatchSendArgs {
    /// Path to a JSON file containing an array of email objects
    #[arg(long)]
    pub file: std::path::PathBuf,
}

// ── API key commands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum ApiKeyCommand {
    #[command(visible_alias = "ls")]
    List,
    #[command(visible_alias = "new")]
    Create(ApiKeyCreateArgs),
    #[command(visible_alias = "rm")]
    Delete(ApiKeyDeleteArgs),
}

#[derive(Args)]
pub struct ApiKeyCreateArgs {
    #[arg(long)]
    pub name: String,
    /// full-access or sending-access
    #[arg(long, default_value = "full-access")]
    pub permission: String,
}

#[derive(Args)]
pub struct ApiKeyDeleteArgs {
    pub id: String,
}

// ── Outbox commands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum OutboxCommand {
    #[command(visible_alias = "ls")]
    List,
    Retry(OutboxRetryArgs),
    Flush,
}

#[derive(Args)]
pub struct OutboxRetryArgs {
    pub id: String,
}

// ── Webhook commands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum WebhookCommand {
    Listen(WebhookListenArgs),
}

#[derive(Args)]
pub struct WebhookListenArgs {
    #[arg(long, default_value = "8080")]
    pub port: u16,
    /// Send desktop notifications for new messages
    #[arg(long)]
    pub notify: bool,
}

// ── Events commands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum EventsCommand {
    #[command(visible_alias = "ls")]
    List(EventsListArgs),
}

#[derive(Args)]
pub struct EventsListArgs {
    #[arg(long)]
    pub message: Option<i64>,
    #[arg(long, default_value = "50")]
    pub limit: usize,
}
