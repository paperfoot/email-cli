use anyhow::{Context, Result};

use crate::app::App;
use crate::cli::*;
use crate::output::Format;

impl App {
    pub fn webhook_listen(&self, args: WebhookListenArgs) -> Result<()> {
        let addr = format!("0.0.0.0:{}", args.port);
        let server = tiny_http::Server::http(&addr)
            .map_err(|e| anyhow::anyhow!("failed to bind {}: {}", addr, e))?;

        if matches!(self.format, Format::Human) {
            eprintln!("listening on http://{}", addr);
            eprintln!("configure Resend webhook to POST to this URL");
        }

        for mut request in server.incoming_requests() {
            if request.method() != &tiny_http::Method::Post {
                let response =
                    tiny_http::Response::from_string("method not allowed").with_status_code(405);
                let _ = request.respond(response);
                continue;
            }

            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                eprintln!("failed to read body: {}", e);
                let response =
                    tiny_http::Response::from_string("bad request").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // Parse the Resend webhook event
            match self.handle_webhook_event(&body) {
                Ok(event_type) => {
                    if matches!(self.format, Format::Human) {
                        eprintln!("event: {}", event_type);
                    }
                    let response =
                        tiny_http::Response::from_string("ok").with_status_code(200);
                    let _ = request.respond(response);
                }
                Err(e) => {
                    eprintln!("error processing event: {}", e);
                    let response =
                        tiny_http::Response::from_string("error").with_status_code(500);
                    let _ = request.respond(response);
                }
            }
        }

        Ok(())
    }

    fn handle_webhook_event(&self, body: &str) -> Result<String> {
        let payload: serde_json::Value =
            serde_json::from_str(body).context("invalid JSON in webhook body")?;

        let event_type = payload
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Extract the email ID from the data object
        let email_id = payload
            .get("data")
            .and_then(|d| d.get("email_id").or_else(|| d.get("id")))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        self.store_event(email_id, &event_type, body)?;

        // If it's a received email event, trigger a sync for that email
        if event_type == "email.received" {
            if let Some(data) = payload.get("data") {
                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                    // Try to fetch and store the received email
                    if let Ok(accounts) = self.list_accounts() {
                        for account in &accounts {
                            if let Ok(client) = self.client_for_profile(&account.profile_name) {
                                if let Ok(detail) = client.get_received_email(id) {
                                    if crate::helpers::received_email_matches_account(
                                        &detail,
                                        &account.email,
                                    ) {
                                        let _ = self
                                            .store_received_message(account, detail.clone());
                                        if let Ok(msg) = self.get_message_by_remote_id(id) {
                                            let _ = self.store_received_attachments(
                                                msg.id,
                                                &detail.attachments,
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(event_type)
    }
}
