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
            dispatch(app, command)
        }
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
            })?,
            InboxCommand::Read(args) => app.inbox_read(args)?,
            InboxCommand::Delete(args) => app.inbox_delete(args)?,
            InboxCommand::Archive(args) => app.inbox_archive(args)?,
            InboxCommand::Search(args) => app.inbox_search(args)?,
            InboxCommand::Purge(args) => app.inbox_purge(args)?,
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
        Command::Audience { command } => match command {
            AudienceCommand::List => app.audience_list()?,
            AudienceCommand::Get(args) => app.audience_get(args)?,
            AudienceCommand::Create(args) => app.audience_create(args)?,
            AudienceCommand::Delete(args) => app.audience_delete(args)?,
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
        Command::AgentInfo | Command::Skill { .. } | Command::Completions { .. } => {
            unreachable!()
        }
    }
    Ok(())
}
