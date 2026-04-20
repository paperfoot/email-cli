use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Validate that a limit value is within Resend's documented range (1-100).
fn parse_resend_limit(raw: &str) -> Result<usize, String> {
    let value: usize = raw
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    if !(1..=100).contains(&value) {
        return Err(format!("must be a number between 1 and 100, got {}", value));
    }
    Ok(value)
}

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
    /// Install/remove a LaunchAgent that runs the daemon at login (macOS)
    Autostart {
        #[command(subcommand)]
        command: AutostartCommand,
    },
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
    /// Manage Resend contacts (use `segment` to group them, `topic` for preferences)
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
    /// List sent emails (Resend GET /emails)
    Email {
        #[command(subcommand)]
        command: EmailCommand,
    },
    /// Manage Resend broadcasts (campaign sends with native unsubscribe wiring)
    Broadcast {
        #[command(subcommand)]
        command: BroadcastCommand,
    },
    /// Manage Resend contact-property schema (define custom fields before assigning values)
    #[command(name = "contact-property")]
    ContactProperty {
        #[command(subcommand)]
        command: ContactPropertyCommand,
    },
    /// Manage Resend topics (granular subscription preferences for broadcasts)
    Topic {
        #[command(subcommand)]
        command: TopicCommand,
    },
    /// Manage Resend segments (replaces the deprecated "audience" noun, Nov 2025)
    Segment {
        #[command(subcommand)]
        command: SegmentCommand,
    },
    /// Self-update from GitHub Releases
    Update {
        /// Check only, don't install
        #[arg(long)]
        check: bool,
    },
    /// View command usage log
    Log(LogArgs),
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
    /// Additional CC recipients (merged with --all's computed CC, deduped)
    #[arg(long)]
    pub cc: Vec<String>,
    /// BCC recipients
    #[arg(long)]
    pub bcc: Vec<String>,
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
pub struct LogArgs {
    /// Number of entries to show
    #[arg(long, default_value = "25")]
    pub limit: usize,
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

#[derive(Subcommand)]
pub enum AutostartCommand {
    /// Install LaunchAgent and load it now
    Install(AutostartInstallArgs),
    /// Remove the LaunchAgent
    Uninstall,
    /// Show LaunchAgent state
    Status(AutostartStatusArgs),
}

#[derive(Args)]
pub struct AutostartInstallArgs {
    /// Account to monitor (default: all accounts)
    #[arg(long)]
    pub account: Option<String>,
    /// Daemon poll interval in seconds
    #[arg(long, default_value = "60")]
    pub interval: u64,
}

#[derive(Args)]
pub struct AutostartStatusArgs {}

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
    /// Mark messages as read or unread
    Mark(InboxMarkArgs),
    #[command(visible_alias = "rm")]
    Delete(InboxDeleteArgs),
    Archive(InboxArchiveArgs),
    Unarchive(InboxUnarchiveArgs),
    /// Show all messages in a conversation thread
    Thread(InboxThreadArgs),
    Search(InboxSearchArgs),
    Purge(InboxPurgeArgs),
    /// Mailbox counts for sidebar / dashboard
    Stats(InboxStatsArgs),
    /// Star / flag messages for quick follow-up
    Star(InboxStarArgs),
    /// Remove the star on messages
    Unstar(InboxStarArgs),
    /// Snooze messages until a future time (they disappear from the inbox
    /// until then, then reappear as unread).
    Snooze(InboxSnoozeArgs),
    /// Cancel a pending snooze so the message reappears now
    Unsnooze(InboxUnsnoozeArgs),
    /// Surface the List-Unsubscribe URL / mailto for a received message so a
    /// client can one-click unsubscribe from marketing mail
    Unsubscribe(InboxUnsubscribeArgs),
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
    /// Show only starred / flagged messages
    #[arg(long)]
    pub starred: bool,
    /// Show only messages currently snoozed into the future. Without this
    /// flag, the list hides snoozed messages until their wake time passes.
    #[arg(long)]
    pub snoozed: bool,
    /// Cursor for pagination: return messages with id < this value
    #[arg(long)]
    pub after: Option<i64>,
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
pub struct InboxMarkArgs {
    /// Message IDs to mark
    pub ids: Vec<i64>,
    /// Mark as read
    #[arg(long, group = "state")]
    pub read: bool,
    /// Mark as unread
    #[arg(long, group = "state")]
    pub unread: bool,
}

#[derive(Args)]
pub struct InboxDeleteArgs {
    /// Message IDs to delete (one or more)
    pub ids: Vec<i64>,
}

#[derive(Args)]
pub struct InboxArchiveArgs {
    /// Message IDs to archive (one or more)
    pub ids: Vec<i64>,
}

#[derive(Args)]
pub struct InboxUnarchiveArgs {
    /// Message IDs to unarchive (one or more)
    pub ids: Vec<i64>,
}

#[derive(Args)]
pub struct InboxThreadArgs {
    /// Any message ID in the thread
    pub id: i64,
}

#[derive(Args)]
pub struct InboxSearchArgs {
    /// Free-text query (matched against subject + body via FTS). Empty string
    /// is allowed when you rely on filter flags only.
    #[arg(default_value = "")]
    pub query: String,
    #[arg(long)]
    pub account: Option<String>,
    #[arg(long, default_value = "25")]
    pub limit: usize,
    /// Filter to messages from this sender (substring match on from_addr)
    #[arg(long)]
    pub from: Option<String>,
    /// Filter to messages sent to this address (substring match on to_json)
    #[arg(long)]
    pub to: Option<String>,
    /// Filter by subject substring
    #[arg(long)]
    pub subject: Option<String>,
    /// Only messages that have at least one attachment
    #[arg(long = "has-attachment")]
    pub has_attachment: bool,
    /// Only unread messages
    #[arg(long)]
    pub unread: bool,
    /// Only starred messages
    #[arg(long)]
    pub starred: bool,
}

#[derive(Args)]
pub struct InboxStatsArgs {
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Args)]
pub struct InboxPurgeArgs {
    /// Delete messages older than this date (YYYY-MM-DD)
    #[arg(long)]
    pub before: String,
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Args)]
pub struct InboxStarArgs {
    /// Message IDs (one or more)
    pub ids: Vec<i64>,
}

#[derive(Args)]
pub struct InboxSnoozeArgs {
    /// Message IDs to snooze
    pub ids: Vec<i64>,
    /// When to wake the message back up. Accepts: `tomorrow`, `tonight`,
    /// `next-week`, `1h`, `4h`, `2d`, `1w`, or ISO-8601 timestamp.
    #[arg(long)]
    pub until: String,
}

#[derive(Args)]
pub struct InboxUnsnoozeArgs {
    pub ids: Vec<i64>,
}

#[derive(Args)]
pub struct InboxUnsubscribeArgs {
    pub id: i64,
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
    /// Number of contacts to return (1-100). Defaults to 50.
    #[arg(long, default_value = "50", value_parser = parse_resend_limit)]
    pub limit: usize,
    /// Cursor: return contacts after this contact id.
    #[arg(long)]
    pub after: Option<String>,
}

#[derive(Args)]
pub struct ContactGetArgs {
    /// Contact id or email address.
    pub id_or_email: String,
}

#[derive(Args)]
pub struct ContactCreateArgs {
    #[arg(long)]
    pub email: String,
    #[arg(long)]
    pub first_name: Option<String>,
    #[arg(long)]
    pub last_name: Option<String>,
    #[arg(long)]
    pub unsubscribed: Option<bool>,
    /// Custom contact properties as a JSON object (e.g. '{"company":"Acme","plan":"pro"}').
    /// Property keys must be defined first via `email-cli contact-property create`.
    #[arg(long, value_name = "JSON")]
    pub properties: Option<String>,
    /// Comma-separated segment ids to add this contact to at create time
    /// (e.g. --segments seg_abc123,seg_def456).
    #[arg(long, value_name = "ID,ID,...")]
    pub segments: Option<String>,
    /// Comma-separated topic subscriptions to set at create time, each formatted as
    /// `topic_id:opt_in` or `topic_id:opt_out` (e.g. --topics top_xxx:opt_in,top_yyy:opt_out).
    #[arg(long, value_name = "TOPIC:STATE,...")]
    pub topics: Option<String>,
}

#[derive(Args)]
pub struct ContactUpdateArgs {
    /// Contact id or email address.
    pub id_or_email: String,
    #[arg(long)]
    pub first_name: Option<String>,
    #[arg(long)]
    pub last_name: Option<String>,
    #[arg(long)]
    pub unsubscribed: Option<bool>,
    /// Custom contact properties as a JSON object. Replaces existing values for the keys present.
    #[arg(long, value_name = "JSON")]
    pub properties: Option<String>,
}

#[derive(Args)]
pub struct ContactDeleteArgs {
    /// Contact id or email address.
    pub id_or_email: String,
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

// ── Email commands (Resend GET /emails wrapper) ────────────────────────────

#[derive(Subcommand)]
pub enum EmailCommand {
    /// List sent emails. Each row includes `last_event` for poll-based status checks.
    #[command(visible_alias = "ls")]
    List(EmailListArgs),
}

#[derive(Args)]
pub struct EmailListArgs {
    /// Number of emails to return (1-100). Defaults to 20.
    #[arg(long, default_value = "20", value_parser = parse_resend_limit)]
    pub limit: usize,
    /// Cursor: return emails created after this email id.
    #[arg(long)]
    pub after: Option<String>,
}

// ── Broadcast commands ─────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum BroadcastCommand {
    #[command(visible_alias = "ls")]
    List,
    #[command(visible_alias = "show")]
    Get(BroadcastGetArgs),
    #[command(visible_alias = "new")]
    Create(BroadcastCreateArgs),
    Update(BroadcastUpdateArgs),
    Send(BroadcastSendArgs),
    #[command(visible_alias = "rm")]
    Delete(BroadcastDeleteArgs),
}

#[derive(Args)]
pub struct BroadcastUpdateArgs {
    pub id: String,
    /// Change the target segment.
    #[arg(long)]
    pub segment_id: Option<String>,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub subject: Option<String>,
    #[arg(long)]
    pub html: Option<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    /// Reply-to address(es), comma-separated.
    #[arg(long)]
    pub reply_to: Option<String>,
    /// Topic ID for per-recipient unsubscribe wiring.
    #[arg(long)]
    pub topic_id: Option<String>,
}

#[derive(Args)]
pub struct BroadcastGetArgs {
    pub id: String,
}

#[derive(Args)]
pub struct BroadcastCreateArgs {
    /// Segment ID to send to. Accepts a segment id or (legacy) audience id.
    #[arg(long, visible_alias = "audience-id")]
    pub segment_id: String,
    /// Sender address (e.g. "Name <sender@example.com>").
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub subject: String,
    /// HTML body. Use `{{{RESEND_UNSUBSCRIBE_URL}}}` for the auto-injected unsubscribe link.
    #[arg(long)]
    pub html: Option<String>,
    /// Plain text body.
    #[arg(long)]
    pub text: Option<String>,
    /// Internal name for the broadcast (not shown to recipients).
    #[arg(long)]
    pub name: Option<String>,
    /// Reply-to address(es), comma-separated.
    #[arg(long)]
    pub reply_to: Option<String>,
    /// Topic ID to scope this broadcast to (drives per-recipient unsubscribe URL).
    #[arg(long)]
    pub topic_id: Option<String>,
    /// Schedule send (ISO-8601 / RFC-3339 timestamp, or natural language per Resend).
    #[arg(long)]
    pub scheduled_at: Option<String>,
    /// Send the broadcast immediately after creation (single API call).
    #[arg(long)]
    pub send: bool,
}

#[derive(Args)]
pub struct BroadcastSendArgs {
    pub id: String,
    /// Optional ISO-8601 / RFC-3339 timestamp to schedule the send.
    #[arg(long)]
    pub scheduled_at: Option<String>,
}

#[derive(Args)]
pub struct BroadcastDeleteArgs {
    pub id: String,
}

// ── Contact-property schema commands ───────────────────────────────────────

#[derive(Subcommand)]
pub enum ContactPropertyCommand {
    #[command(visible_alias = "ls")]
    List,
    #[command(visible_alias = "show")]
    Get(ContactPropertyGetArgs),
    #[command(visible_alias = "new")]
    Create(ContactPropertyCreateArgs),
    Update(ContactPropertyUpdateArgs),
    #[command(visible_alias = "rm")]
    Delete(ContactPropertyDeleteArgs),
}

#[derive(Args)]
pub struct ContactPropertyUpdateArgs {
    pub id: String,
    /// New fallback value. Pass numbers as their text representation; we'll send them as
    /// numbers when `--as-number` is set, otherwise as strings.
    #[arg(long)]
    pub fallback: Option<String>,
    /// Treat `--fallback` as a number rather than a string.
    #[arg(long)]
    pub as_number: bool,
}

#[derive(Args)]
pub struct ContactPropertyGetArgs {
    pub id: String,
}

#[derive(Args)]
pub struct ContactPropertyCreateArgs {
    /// Property key. Alphanumeric and underscores only, max 50 chars.
    #[arg(long)]
    pub key: String,
    /// Property type: "string" or "number".
    #[arg(long, default_value = "string")]
    pub property_type: String,
    /// Optional fallback value when the property is not set on a contact (must match type).
    #[arg(long)]
    pub fallback: Option<String>,
}

#[derive(Args)]
pub struct ContactPropertyDeleteArgs {
    pub id: String,
}

// ── Topic commands ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum TopicCommand {
    #[command(visible_alias = "ls")]
    List,
    #[command(visible_alias = "show")]
    Get(TopicGetArgs),
    #[command(visible_alias = "new")]
    Create(TopicCreateArgs),
    Update(TopicUpdateArgs),
    #[command(visible_alias = "rm")]
    Delete(TopicDeleteArgs),
    /// Subscribe / unsubscribe a contact to a topic
    ContactSet(TopicContactSetArgs),
    /// List a contact's topic subscriptions
    ContactList(TopicContactListArgs),
}

#[derive(Args)]
pub struct TopicUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    /// Default subscription state for new contacts: "opt_in" or "opt_out".
    #[arg(long)]
    pub default_subscription: Option<String>,
    /// "public" or "private" — controls visibility on the hosted preference page.
    #[arg(long)]
    pub visibility: Option<String>,
}

#[derive(Args)]
pub struct TopicContactListArgs {
    /// Contact id or email
    #[arg(long)]
    pub contact: String,
}

#[derive(Args)]
pub struct TopicGetArgs {
    pub id: String,
}

#[derive(Args)]
pub struct TopicCreateArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub description: Option<String>,
    /// Default subscription state for new contacts: "opt_in" or "opt_out".
    #[arg(long)]
    pub default_subscription: Option<String>,
    /// "public" or "private" — controls visibility on the hosted preference page.
    #[arg(long)]
    pub visibility: Option<String>,
}

#[derive(Args)]
pub struct TopicDeleteArgs {
    pub id: String,
}

#[derive(Args)]
pub struct TopicContactSetArgs {
    /// Contact id or email
    #[arg(long)]
    pub contact: String,
    /// Topic id
    #[arg(long)]
    pub topic: String,
    /// Subscription state: "opt_in" or "opt_out".
    #[arg(long)]
    pub subscription: String,
}

// ── Segment commands (Audiences renamed to Segments in November 2025) ─────

#[derive(Subcommand)]
pub enum SegmentCommand {
    #[command(visible_alias = "ls")]
    List,
    #[command(visible_alias = "show")]
    Get(SegmentGetArgs),
    #[command(visible_alias = "new")]
    Create(SegmentCreateArgs),
    #[command(visible_alias = "rm")]
    Delete(SegmentDeleteArgs),
    /// Add a contact to a segment
    ContactAdd(SegmentContactArgs),
    /// Remove a contact from a segment
    ContactRemove(SegmentContactArgs),
    /// List the segments a contact belongs to
    ContactList(SegmentContactListArgs),
    /// List the contacts in a segment
    Contacts(SegmentContactsArgs),
}

#[derive(Args)]
pub struct SegmentContactsArgs {
    /// Segment id
    pub id: String,
}

#[derive(Args)]
pub struct SegmentGetArgs {
    pub id: String,
}

#[derive(Args)]
pub struct SegmentCreateArgs {
    #[arg(long)]
    pub name: String,
}

#[derive(Args)]
pub struct SegmentDeleteArgs {
    pub id: String,
}

#[derive(Args)]
pub struct SegmentContactArgs {
    /// Contact id or email
    #[arg(long)]
    pub contact: String,
    /// Segment id
    #[arg(long)]
    pub segment: String,
}

#[derive(Args)]
pub struct SegmentContactListArgs {
    /// Contact id or email
    #[arg(long)]
    pub contact: String,
}
