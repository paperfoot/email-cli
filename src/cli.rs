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
    #[arg(long)]
    pub account: Option<String>,
    #[arg(long, required = true)]
    pub to: Vec<String>,
    #[arg(long)]
    pub cc: Vec<String>,
    #[arg(long)]
    pub bcc: Vec<String>,
    #[arg(long, default_value = "")]
    pub subject: String,
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
    #[arg(long)]
    pub account: Option<String>,
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

#[derive(Subcommand)]
pub enum DraftCommand {
    Create(DraftCreateArgs),
    List(DraftListArgs),
    Show(DraftShowArgs),
    Send(DraftSendArgs),
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
}

#[derive(Subcommand)]
pub enum InboxCommand {
    Ls(InboxListArgs),
    Read(InboxReadArgs),
}

#[derive(Args)]
pub struct InboxListArgs {
    #[arg(long)]
    pub account: Option<String>,
    #[arg(long, default_value = "25")]
    pub limit: usize,
    #[arg(long)]
    pub unread: bool,
}

#[derive(Args)]
pub struct InboxReadArgs {
    pub id: i64,
    #[arg(long)]
    pub mark_read: bool,
}

#[derive(Subcommand)]
pub enum AttachmentsCommand {
    List(AttachmentListArgs),
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
