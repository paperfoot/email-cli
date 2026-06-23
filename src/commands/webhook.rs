use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::app::App;
use crate::cli::*;
use crate::helpers::send_desktop_notification;
use crate::output::Format;

/// Header the listener checks against the configured shared secret.
const WEBHOOK_SECRET_HEADER: &str = "X-Webhook-Secret";

/// Max accepted clock skew for a Svix-signed webhook, in seconds. Bounds replay.
const SVIX_TOLERANCE_SECS: i64 = 5 * 60;

type HmacSha256 = Hmac<Sha256>;

impl App {
    pub fn webhook_listen(&self, args: WebhookListenArgs) -> Result<()> {
        let notify = args.notify;

        // Resolve the shared secret. Env wins over file; trim whitespace so
        // newline-terminated files (the common case) work without surprises.
        let secret = resolve_secret(args.secret_env.as_deref(), args.secret_file.as_deref())?;

        // Resolve the Svix signing secret — the *real* auth for a Resend
        // webhook. Resend signs every delivery (svix-id/-timestamp/-signature),
        // so this is what proves an event genuinely came from Resend; the
        // X-Webhook-Secret shared secret is only a coarse extra gate.
        let signing_secret = resolve_secret(
            args.signing_secret_env.as_deref(),
            args.signing_secret_file.as_deref(),
        )?;

        // Opening to the LAN with NO auth at all is unsafe. 127.0.0.1 + no-auth
        // is a safe local baseline; a public bind needs either a signing secret
        // (real) or the shared secret (coarse).
        if is_public_bind(&args.host) && secret.is_none() && signing_secret.is_none() {
            anyhow::bail!(
                "refusing to start: --host {} exposes the webhook to the LAN but no auth is set. \
                 Pass --signing-secret-env <VAR> (Resend whsec_... signing secret) to verify \
                 signatures, or --secret-env <VAR> for a shared-secret header.",
                args.host
            );
        }

        if signing_secret.is_none() && secret.is_none() && matches!(self.format, Format::Human) {
            eprintln!(
                "WARNING: webhook listener has no signing secret configured, so Resend signatures \
                 are NOT verified. Anyone who can reach {} can POST forged events. Pass \
                 --signing-secret-env <VAR> with your Resend whsec_... secret to lock it down.",
                args.host
            );
        }

        let addr = format!("{}:{}", args.host, args.port);
        let server = tiny_http::Server::http(&addr)
            .map_err(|e| anyhow::anyhow!("failed to bind {}: {}", addr, e))?;

        if matches!(self.format, Format::Human) {
            eprintln!("listening on http://{}", addr);
            eprintln!("configure Resend webhook to POST to this URL");
            if signing_secret.is_some() {
                eprintln!("auth: verifying Svix signatures (svix-signature header)");
            }
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

            // Snapshot the Svix headers before consuming the body (signature is
            // computed over the raw bytes, so capture them first).
            let svix_id = header_value(&request, "svix-id");
            let svix_timestamp = header_value(&request, "svix-timestamp");
            let svix_signature = header_value(&request, "svix-signature");

            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                eprintln!("failed to read body: {}", e);
                let response =
                    tiny_http::Response::from_string("bad request").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // Svix signature verification — proves the event came from Resend.
            if let Some(signing) = signing_secret.as_deref() {
                let now = unix_now();
                let ok = match (&svix_id, &svix_timestamp, &svix_signature) {
                    (Some(id), Some(ts), Some(sig)) => {
                        verify_svix(signing, id, ts, sig, &body, now)
                    }
                    _ => false,
                };
                if !ok {
                    if matches!(self.format, Format::Human) {
                        eprintln!("rejected request: invalid or missing Svix signature");
                    }
                    let response =
                        tiny_http::Response::from_string("unauthorized").with_status_code(401);
                    let _ = request.respond(response);
                    continue;
                }
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

/// Case-insensitive header lookup off a tiny_http request.
fn header_value(request: &tiny_http::Request, name: &str) -> Option<String> {
    // tiny_http's `equiv` only accepts &'static str, so compare the rendered
    // field name case-insensitively for a runtime header name.
    request
        .headers()
        .iter()
        .find(|h| format!("{}", h.field).eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Verify a Svix-signed webhook (the scheme Resend uses). The signed content is
/// `{svix-id}.{svix-timestamp}.{body}`, HMAC-SHA256'd with the base64-decoded
/// secret (after stripping the `whsec_` prefix), base64-encoded. The
/// `svix-signature` header is a space-separated list of `v1,<sig>` entries; any
/// constant-time match passes. Stale/early timestamps are rejected to bound replay.
fn verify_svix(
    secret: &str,
    svix_id: &str,
    svix_timestamp: &str,
    svix_signature: &str,
    body: &str,
    now: i64,
) -> bool {
    let ts: i64 = match svix_timestamp.trim().parse() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if (now - ts).abs() > SVIX_TOLERANCE_SECS {
        return false;
    }

    let key_part = secret.strip_prefix("whsec_").unwrap_or(secret);
    let key = match BASE64.decode(key_part.trim()) {
        Ok(k) => k,
        Err(_) => return false,
    };

    let signed = format!("{svix_id}.{svix_timestamp}.{body}");
    let mut mac = match HmacSha256::new_from_slice(&key) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(signed.as_bytes());
    let expected = BASE64.encode(mac.finalize().into_bytes());

    svix_signature.split(' ').any(|entry| {
        let sig = entry.split_once(',').map(|(_, s)| s).unwrap_or(entry);
        constant_time_eq(sig.as_bytes(), expected.as_bytes())
    })
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

    // ── Svix signature verification ──────────────────────────────────────

    /// A valid `whsec_`-prefixed secret (the key part must be valid base64).
    fn test_secret() -> String {
        format!("whsec_{}", BASE64.encode(b"super-secret-signing-key"))
    }

    fn sign(secret: &str, id: &str, ts: &str, body: &str) -> String {
        let key_part = secret.strip_prefix("whsec_").unwrap();
        let key = BASE64.decode(key_part).unwrap();
        let mut mac = HmacSha256::new_from_slice(&key).unwrap();
        mac.update(format!("{id}.{ts}.{body}").as_bytes());
        format!("v1,{}", BASE64.encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn svix_accepts_valid_signature() {
        let secret = test_secret();
        let (id, ts, body) = ("msg_1", "1700000000", r#"{"type":"email.received"}"#);
        let sig = sign(&secret, id, ts, body);
        assert!(verify_svix(&secret, id, ts, &sig, body, 1700000000));
    }

    #[test]
    fn svix_rejects_tampered_body() {
        let secret = test_secret();
        let (id, ts, body) = ("msg_1", "1700000000", r#"{"type":"email.received"}"#);
        let sig = sign(&secret, id, ts, body);
        assert!(!verify_svix(&secret, id, ts, &sig, r#"{"type":"forged"}"#, 1700000000));
    }

    #[test]
    fn svix_rejects_stale_timestamp() {
        let secret = test_secret();
        let (id, ts, body) = ("msg_1", "1700000000", "{}");
        let sig = sign(&secret, id, ts, body);
        // now is an hour past the signed timestamp → outside tolerance.
        assert!(!verify_svix(&secret, id, ts, &sig, body, 1700000000 + 3600));
    }

    #[test]
    fn svix_rejects_wrong_secret() {
        let good = test_secret();
        let (id, ts, body) = ("msg_1", "1700000000", "{}");
        let sig = sign(&good, id, ts, body);
        let other = format!("whsec_{}", BASE64.encode(b"a-different-key-entirely"));
        assert!(!verify_svix(&other, id, ts, &sig, body, 1700000000));
    }

    #[test]
    fn svix_accepts_when_one_of_several_signatures_matches() {
        let secret = test_secret();
        let (id, ts, body) = ("msg_1", "1700000000", "{}");
        let valid = sign(&secret, id, ts, body);
        // Resend may send multiple space-separated entries (key rotation).
        let header = format!("v1,bogussignature {valid}");
        assert!(verify_svix(&secret, id, ts, &header, body, 1700000000));
    }
}
