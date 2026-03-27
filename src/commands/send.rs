use anyhow::{Result, bail};
use std::collections::HashMap;

use crate::app::App;
use crate::cli::{ComposeArgs, ReplyArgs, SendArgs};
use crate::helpers::{
    append_signature_html, append_signature_text, build_idempotency_key, build_send_attachments,
    ensure_reply_account_matches, format_sender, normalize_email, normalize_emails,
    now_timestamp, read_optional_content, reply_headers_for_message, reply_recipients,
    reply_subject,
};
use crate::http::fetch_sent_detail;
use crate::models::{
    MessageRecord, ReplyHeaders, ResolvedCompose, SendEmailRequest, SentEmail,
};
use crate::output::print_success_or;

impl App {
    pub fn send(&self, args: SendArgs) -> Result<()> {
        let compose = self.resolve_compose(args.compose)?;
        let message = self.send_compose(compose, None)?;

        print_success_or(self.format, &message, |message| {
            println!(
                "sent message {} from {} to {}",
                message.id,
                message.account_email,
                message.to.join(", ")
            );
        });

        Ok(())
    }

    pub fn reply(&self, args: ReplyArgs) -> Result<()> {
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

        print_success_or(self.format, &message, |message| {
            println!("replied with message {}", message.id);
        });

        Ok(())
    }

    pub fn resolve_compose(&self, compose: ComposeArgs) -> Result<ResolvedCompose> {
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

    pub fn send_compose(
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
}
