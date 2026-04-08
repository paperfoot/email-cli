mod app;
mod cli;
mod commands;
mod db;
mod error;
mod helpers;
mod http;
mod models;
mod output;
mod resend;

use clap::Parser;
use std::io::IsTerminal;

use crate::app::App;
use crate::cli::*;
use crate::error::CliError;
use crate::helpers::default_db_path;
use crate::output::Format;

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // Help and version are not errors — exit 0.
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
            ) {
                if !std::io::stdout().is_terminal() {
                    let envelope = serde_json::json!({
                        "version": "1",
                        "status": "success",
                        "data": { "usage": err.to_string().trim_end() },
                    });
                    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
                    std::process::exit(0);
                }
                err.exit();
            }

            let format = Format::detect(false);
            output::print_clap_error(format, err);
            std::process::exit(3);
        }
    };

    let format = Format::detect(cli.json);

    let result = run(cli.command, cli.db, format);
    if let Err(err) = result {
        output::print_error(format, &err);
        std::process::exit(err.exit_code());
    }
}

fn run(command: Command, db: Option<std::path::PathBuf>, format: Format) -> Result<(), CliError> {
    match command {
        Command::AgentInfo => {
            commands::agent_info::run(format);
            Ok(())
        }
        Command::Skill { action } => match action {
            SkillAction::Install => commands::skill::install(format).map_err(CliError::from),
            SkillAction::Status => commands::skill::status(format).map_err(CliError::from),
        },
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut <Cli as clap::CommandFactory>::command(),
                "email-cli",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        _ => {
            let db_path = db.unwrap_or(default_db_path()?);
            let app = App::new(db_path, format)?;
            let cmd_name = command_name(&command);
            let cmd_args = command_args(&command);
            app.log_command(&cmd_name, &cmd_args);
            let result = dispatch(app, command);
            result
        }
    }
}

fn command_name(command: &Command) -> String {
    match command {
        Command::Profile { .. } => "profile",
        Command::Account { .. } => "account",
        Command::Signature { .. } => "signature",
        Command::Send(_) => "send",
        Command::Reply(_) => "reply",
        Command::Forward(_) => "forward",
        Command::Daemon(_) => "daemon",
        Command::Update { .. } => "update",
        Command::Log(_) => "log",
        Command::Sync(_) => "sync",
        Command::Inbox { .. } => "inbox",
        Command::Attachments { .. } => "attachments",
        Command::Domain { .. } => "domain",
        Command::Contact { .. } => "contact",
        Command::Batch { .. } => "batch",
        Command::ApiKey { .. } => "api-key",
        Command::Outbox { .. } => "outbox",
        Command::Webhook { .. } => "webhook",
        Command::Events { .. } => "events",
        Command::Email { .. } => "email",
        Command::Broadcast { .. } => "broadcast",
        Command::ContactProperty { .. } => "contact-property",
        Command::Topic { .. } => "topic",
        Command::Segment { .. } => "segment",
        Command::Draft { .. } => "draft",
        Command::AgentInfo => "agent-info",
        Command::Skill { .. } => "skill",
        Command::Completions { .. } => "completions",
    }
    .to_string()
}

fn command_args(command: &Command) -> String {
    // Capture subcommand details without sensitive content
    match command {
        Command::Inbox { command } => match command {
            InboxCommand::List(a) => format!("list --limit {} {}", a.limit, a.account.as_deref().unwrap_or("")),
            InboxCommand::Sync(a) => format!("sync {}", a.account.as_deref().unwrap_or("")),
            InboxCommand::Read(a) => format!("read {}", a.id),
            InboxCommand::Mark(a) => format!("mark {:?} {}", a.ids, if a.unread { "--unread" } else { "--read" }),
            InboxCommand::Delete(a) => format!("delete {:?}", a.ids),
            InboxCommand::Archive(a) => format!("archive {:?}", a.ids),
            InboxCommand::Unarchive(a) => format!("unarchive {:?}", a.ids),
            InboxCommand::Thread(a) => format!("thread {}", a.id),
            InboxCommand::Search(a) => format!("search \"{}\"", a.query),
            InboxCommand::Purge(a) => format!("purge --before {}", a.before),
            InboxCommand::Stats(a) => format!("stats {}", a.account.as_deref().unwrap_or("")),
        },
        Command::Send(a) => format!("--to {:?} --subject \"{}\"", a.compose.to, a.compose.subject),
        Command::Reply(a) => format!("{}{}", a.message_id, if a.all { " --all" } else { "" }),
        Command::Forward(a) => format!("{} --to {:?}", a.message_id, a.to),
        Command::Sync(a) => format!("{}", a.account.as_deref().unwrap_or("all")),
        Command::Events { command } => match command {
            EventsCommand::List(a) => format!("list{}", a.message.map(|m| format!(" --message {}", m)).unwrap_or_default()),
        },
        _ => String::new(),
    }
}

fn dispatch(app: App, command: Command) -> Result<(), CliError> {
    match command {
        Command::Profile { command } => match command {
            ProfileCommand::Add(args) => app.profile_add(args)?,
            ProfileCommand::List => app.profile_list()?,
            ProfileCommand::Test(args) => app.profile_test(args)?,
        },
        Command::Account { command } => match command {
            AccountCommand::Add(args) => app.account_add(args)?,
            AccountCommand::List => app.account_list()?,
            AccountCommand::Use(args) => app.account_use(args)?,
        },
        Command::Signature { command } => match command {
            SignatureCommand::Set(args) => app.signature_set(args)?,
            SignatureCommand::Show(args) => app.signature_show(args)?,
        },
        Command::Send(args) => app.send(args)?,
        Command::Reply(args) => app.reply(args)?,
        Command::Forward(args) => app.forward(args)?,
        Command::Daemon(args) => app.daemon(args)?,
        Command::Update { check } => app.update(check)?,
        Command::Log(args) => {
            let entries = app.get_command_log(args.limit)?;
            crate::output::print_success_or(app.format, &entries, |entries| {
                for e in entries {
                    let exit = e.exit_code.map(|c| format!(" (exit {})", c)).unwrap_or_default();
                    println!("{} | {:<12} {}{}", e.created_at, e.command, e.args, exit);
                }
                if entries.is_empty() {
                    println!("no commands logged yet");
                }
            });
        }
        Command::Draft { command } => match command {
            DraftCommand::Create(args) => app.draft_create(args)?,
            DraftCommand::List(args) => app.draft_list(args)?,
            DraftCommand::Show(args) => app.draft_show(args)?,
            DraftCommand::Send(args) => app.draft_send(args)?,
            DraftCommand::Edit(args) => app.draft_edit(args)?,
            DraftCommand::Delete(args) => app.draft_delete(args)?,
        },
        Command::Sync(args) => app.sync(args)?,
        Command::Inbox { command } => match command {
            InboxCommand::List(args) => app.inbox_list(args)?,
            InboxCommand::Sync(args) => app.sync(SyncArgs {
                account: args.account,
                limit: args.limit,
                watch: false,
                interval: None,
                notify: false,
            })?,
            InboxCommand::Read(args) => app.inbox_read(args)?,
            InboxCommand::Mark(args) => app.inbox_mark(args)?,
            InboxCommand::Delete(args) => app.inbox_delete(args)?,
            InboxCommand::Archive(args) => app.inbox_archive(args)?,
            InboxCommand::Unarchive(args) => app.inbox_unarchive(args)?,
            InboxCommand::Thread(args) => app.inbox_thread(args)?,
            InboxCommand::Search(args) => app.inbox_search(args)?,
            InboxCommand::Purge(args) => app.inbox_purge(args)?,
            InboxCommand::Stats(args) => app.inbox_stats(args)?,
        },
        Command::Attachments { command } => match command {
            AttachmentsCommand::List(args) => app.attachments_list(args)?,
            AttachmentsCommand::Get(args) => app.attachments_get(args)?,
        },
        Command::Domain { command } => match command {
            DomainCommand::List => app.domain_list()?,
            DomainCommand::Get(args) => app.domain_get(args)?,
            DomainCommand::Create(args) => app.domain_create(args)?,
            DomainCommand::Verify(args) => app.domain_verify(args)?,
            DomainCommand::Delete(args) => app.domain_delete(args)?,
            DomainCommand::Update(args) => app.domain_update(args)?,
        },
        Command::Contact { command } => match command {
            ContactCommand::List(args) => app.contact_list(args)?,
            ContactCommand::Get(args) => app.contact_get(args)?,
            ContactCommand::Create(args) => app.contact_create(args)?,
            ContactCommand::Update(args) => app.contact_update(args)?,
            ContactCommand::Delete(args) => app.contact_delete(args)?,
        },
        Command::Batch { command } => match command {
            BatchCommand::Send(args) => app.batch_send(args)?,
        },
        Command::ApiKey { command } => match command {
            ApiKeyCommand::List => app.api_key_list()?,
            ApiKeyCommand::Create(args) => app.api_key_create(args)?,
            ApiKeyCommand::Delete(args) => app.api_key_delete(args)?,
        },
        Command::Outbox { command } => match command {
            OutboxCommand::List => app.outbox_list()?,
            OutboxCommand::Retry(args) => app.outbox_retry(args)?,
            OutboxCommand::Flush => app.outbox_flush()?,
        },
        Command::Webhook { command } => match command {
            WebhookCommand::Listen(args) => app.webhook_listen(args)?,
        },
        Command::Events { command } => match command {
            EventsCommand::List(args) => app.events_list(args)?,
        },
        Command::Email { command } => match command {
            EmailCommand::List(args) => app.email_list(args)?,
        },
        Command::Broadcast { command } => match command {
            BroadcastCommand::List => app.broadcast_list()?,
            BroadcastCommand::Get(args) => app.broadcast_get(args)?,
            BroadcastCommand::Create(args) => app.broadcast_create(args)?,
            BroadcastCommand::Update(args) => app.broadcast_update(args)?,
            BroadcastCommand::Send(args) => app.broadcast_send(args)?,
            BroadcastCommand::Delete(args) => app.broadcast_delete(args)?,
        },
        Command::ContactProperty { command } => match command {
            ContactPropertyCommand::List => app.contact_property_list()?,
            ContactPropertyCommand::Get(args) => app.contact_property_get(args)?,
            ContactPropertyCommand::Create(args) => app.contact_property_create(args)?,
            ContactPropertyCommand::Update(args) => app.contact_property_update(args)?,
            ContactPropertyCommand::Delete(args) => app.contact_property_delete(args)?,
        },
        Command::Topic { command } => match command {
            TopicCommand::List => app.topic_list()?,
            TopicCommand::Get(args) => app.topic_get(args)?,
            TopicCommand::Create(args) => app.topic_create(args)?,
            TopicCommand::Update(args) => app.topic_update(args)?,
            TopicCommand::Delete(args) => app.topic_delete(args)?,
            TopicCommand::ContactSet(args) => app.topic_contact_set(args)?,
            TopicCommand::ContactList(args) => app.topic_contact_list(args)?,
        },
        Command::Segment { command } => match command {
            SegmentCommand::List => app.segment_list()?,
            SegmentCommand::Get(args) => app.segment_get(args)?,
            SegmentCommand::Create(args) => app.segment_create(args)?,
            SegmentCommand::Delete(args) => app.segment_delete(args)?,
            SegmentCommand::ContactAdd(args) => app.segment_contact_add(args)?,
            SegmentCommand::ContactRemove(args) => app.segment_contact_remove(args)?,
            SegmentCommand::ContactList(args) => app.segment_contact_list(args)?,
            SegmentCommand::Contacts(args) => app.segment_contacts(args)?,
        },
        Command::AgentInfo | Command::Skill { .. } | Command::Completions { .. } => {
            unreachable!()
        }
    }
    Ok(())
}
