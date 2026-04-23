use anyhow::{Context, Result};

use crate::app::App;
use crate::cli::*;
use crate::helpers::send_desktop_notification;
use crate::output::Format;

/// Header the listener checks against the configured shared secret.
const WEBHOOK_SECRET_HEADER: &str = "X-Webhook-Secret";

impl App {
    pub fn webhook_listen(&self, args: WebhookListenArgs) -> Result<()> {
        let notify = args.notify;

        // Resolve the shared secret. Env wins over file; trim whitespace so
        // newline-terminated files (the common case) work without surprises.
        let secret = resolve_secret(args.secret_env.as_deref(), args.secret_file.as_deref())?;

        // If the user explicitly opens the listener to the LAN (0.0.0.0 or
        // :: for IPv6), require a secret. Defaulting to 127.0.0.1 + no-secret
        // is a safe v1 baseline; 0.0.0.0 + no-secret is not.
        if is_public_bind(&args.host) && secret.is_none() {
            anyhow::bail!(
                "refusing to start: --host {} exposes the webhook to the LAN but no shared secret is set. \
                 Pass --secret-env <VAR> (or --secret-file <PATH>) to enable auth.",
                args.host
            );
        }

        if secret.is_none() && matches!(self.format, Format::Human) {
            eprintln!(
                "WARNING: webhook listener has no shared secret configured. \
                 Anyone who can reach {} can POST events. Pass --secret-env to lock it down.",
                args.host
            );
        }

        let addr = format!("{}:{}", args.host, args.port);
        let server = tiny_http::Server::http(&addr)
            .map_err(|e| anyhow::anyhow!("failed to bind {}: {}", addr, e))?;

        if matches!(self.format, Format::Human) {
            eprintln!("listening on http://{}", addr);
            eprintln!("configure Resend webhook to POST to this URL");
            if secret.is_some() {
                eprintln!("auth: requiring {} header", WEBHOOK_SECRET_HEADER);
            }
        }

        for mut request in server.incoming_requests() {
            if request.method() != &tiny_http::Method::Post {
                let response =
                    tiny_http::Response::from_string("method not allowed").with_status_code(405);
                let _ = request.respond(response);
                continue;
            }

            // Auth gate — check header BEFORE reading the body so an
            // unauthenticated client can't use us to burn memory on a huge
            // payload.
            if let Some(expected) = secret.as_deref() {
                let provided = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv(WEBHOOK_SECRET_HEADER))
                    .map(|h| h.value.as_str());
                let authorized = match provided {
                    Some(v) => constant_time_eq(v.as_bytes(), expected.as_bytes()),
                    None => false,
                };
                if !authorized {
                    if matches!(self.format, Format::Human) {
                        eprintln!("rejected request: missing or invalid {}", WEBHOOK_SECRET_HEADER);
                    }
                    let response =
                        tiny_http::Response::from_string("unauthorized").with_status_code(401);
                    let _ = request.respond(response);
                    continue;
                }
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
            match self.handle_webhook_event(&body, notify) {
                Ok(event_type) => {
                    if matches!(self.format, Format::Human) {
                        eprintln!("event: {}", event_type);
                    }
                    let response = tiny_http::Response::from_string("ok").with_status_code(200);
                    let _ = request.respond(response);
                }
                Err(e) => {
                    eprintln!("error processing event: {}", e);
                    let response = tiny_http::Response::from_string("error").with_status_code(500);
                    let _ = request.respond(response);
                }
            }
        }

        Ok(())
    }

    fn handle_webhook_event(&self, body: &str, notify: bool) -> Result<String> {
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
        if event_type == "email.received"
            && let Some(data) = payload.get("data")
            && let Some(id) = data.get("id").and_then(|v| v.as_str())
        {
            // Try to fetch and store the received email
            if let Ok(accounts) = self.list_accounts() {
                for account in &accounts {
                    if let Ok(client) = self.client_for_profile(&account.profile_name)
                        && let Ok(detail) = client.get_received_email(id)
                        && crate::helpers::received_email_matches_account(&detail, &account.email)
                    {
                        let _ = self.store_received_message(account, detail.clone());
                        if let Ok(msg) = self.get_message_by_remote_id(id) {
                            let _ = self.store_received_attachments(msg.id, &detail.attachments);
                        }
                        if notify {
                            let from = detail.from.as_deref().unwrap_or("unknown");
                            let subject = detail.subject.as_deref().unwrap_or("(no subject)");
                            send_desktop_notification(
                                &format!("New email to {}", account.email),
                                &format!("From: {}\n{}", from, subject),
                            );
                        }
                        break;
                    }
                }
            }
        }

        Ok(event_type)
    }
}

/// Look up the shared secret, preferring env-var over file per our CLI
/// contract. Returns `Ok(None)` when the user hasn't asked for auth.
fn resolve_secret(secret_env: Option<&str>, secret_file: Option<&str>) -> Result<Option<String>> {
    if let Some(var) = secret_env {
        let value = std::env::var(var).with_context(|| {
            format!(
                "--secret-env references environment variable `{}` but it is not set",
                var
            )
        })?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            anyhow::bail!("environment variable `{}` is empty", var);
        }
        return Ok(Some(trimmed.to_string()));
    }
    if let Some(path) = secret_file {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read --secret-file {}", path))?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--secret-file {} is empty", path);
        }
        return Ok(Some(trimmed.to_string()));
    }
    Ok(None)
}

/// True when the host string binds to an interface that LAN peers can reach.
/// Keeps us conservative — anything but `localhost`, `127.x.y.z`, or `::1`
/// is treated as public.
fn is_public_bind(host: &str) -> bool {
    let h = host.trim();
    if h.eq_ignore_ascii_case("localhost") {
        return false;
    }
    if h == "::1" || h == "[::1]" {
        return false;
    }
    if let Ok(ip) = h.parse::<std::net::Ipv4Addr>() {
        return !ip.is_loopback();
    }
    if let Ok(ip) = h.parse::<std::net::Ipv6Addr>() {
        return !ip.is_loopback();
    }
    // Unknown hostnames (e.g. `mybox.lan`) — assume reachable.
    true
}

/// Constant-time byte comparison. Avoids timing side channels when
/// validating the shared secret; standard library `==` would short-circuit
/// on the first mismatch.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_bind_detects_wildcard_v4() {
        assert!(is_public_bind("0.0.0.0"));
    }

    #[test]
    fn public_bind_detects_wildcard_v6() {
        assert!(is_public_bind("::"));
    }

    #[test]
    fn public_bind_allows_loopback_v4() {
        assert!(!is_public_bind("127.0.0.1"));
        assert!(!is_public_bind("127.1.2.3"));
    }

    #[test]
    fn public_bind_allows_loopback_v6() {
        assert!(!is_public_bind("::1"));
        assert!(!is_public_bind("[::1]"));
    }

    #[test]
    fn public_bind_allows_localhost_hostname() {
        assert!(!is_public_bind("localhost"));
        assert!(!is_public_bind("LOCALHOST"));
    }

    #[test]
    fn public_bind_flags_unknown_host() {
        assert!(is_public_bind("mybox.lan"));
        assert!(is_public_bind("192.168.1.10"));
    }

    #[test]
    fn constant_time_eq_matches_on_equal() {
        assert!(constant_time_eq(b"shh-secret", b"shh-secret"));
    }

    #[test]
    fn constant_time_eq_rejects_length_mismatch() {
        assert!(!constant_time_eq(b"short", b"shorter"));
    }

    #[test]
    fn constant_time_eq_rejects_different_payload() {
        assert!(!constant_time_eq(b"aaaaaaa", b"aaaaaab"));
    }

    #[test]
    fn resolve_secret_reads_env_var() {
        // Use a uniquely-named var so we don't collide with the host shell.
        let var = "EMAIL_CLI_TEST_WEBHOOK_SECRET_OK";
        unsafe { std::env::set_var(var, " hunter2\n") };
        let got = resolve_secret(Some(var), None).unwrap();
        assert_eq!(got.as_deref(), Some("hunter2"));
        unsafe { std::env::remove_var(var) };
    }

    #[test]
    fn resolve_secret_errors_when_env_missing() {
        let var = "EMAIL_CLI_TEST_WEBHOOK_SECRET_MISSING";
        unsafe { std::env::remove_var(var) };
        assert!(resolve_secret(Some(var), None).is_err());
    }

    #[test]
    fn resolve_secret_none_when_unset() {
        assert!(resolve_secret(None, None).unwrap().is_none());
    }
}
