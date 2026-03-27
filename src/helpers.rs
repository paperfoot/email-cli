use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, SecondsFormat, Utc};
use dirs::data_local_dir;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::models::*;

pub fn default_db_path() -> Result<PathBuf> {
    let base = data_local_dir().unwrap_or(std::env::current_dir()?);
    Ok(base.join("email-cli").join("email-cli.db"))
}

pub fn resolve_api_key(
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

pub fn cleanup_env_value(value: &str) -> String {
    let mut value = value.trim().to_string();
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        value = value[1..value.len() - 1].to_string();
    }
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        value = value[1..value.len() - 1].to_string();
    }
    value.replace("\\n", "").trim().to_string()
}

pub fn read_optional_content(value: Option<String>, path: Option<PathBuf>) -> Result<Option<String>> {
    match (value, path) {
        (Some(text), None) => Ok(Some(text)),
        (None, Some(path)) => fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))
            .map(Some),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => bail!("use either inline content or a file, not both"),
    }
}

pub fn build_send_attachments(paths: &[PathBuf]) -> Result<Vec<SendAttachment>> {
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

pub fn normalize_email(value: &str) -> String {
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

pub fn normalize_emails(values: &[String]) -> Vec<String> {
    values.iter().map(|value| normalize_email(value)).collect()
}

pub fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).context("failed to serialize json")
}

pub fn from_json<T: DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).context("failed to parse json")
}

pub fn format_sender(display_name: Option<&str>, email: &str) -> String {
    match display_name {
        Some(name) if !name.trim().is_empty() => format!("{} <{}>", name.trim(), email),
        _ => email.to_string(),
    }
}

pub fn append_signature_text(body: Option<&str>, signature: &str) -> String {
    let body = body.unwrap_or("").trim_end();
    let signature = signature.trim();
    if body.is_empty() {
        signature.to_string()
    } else {
        format!("{body}\n\n-- \n{signature}")
    }
}

pub fn append_signature_html(body: Option<&str>, signature: &str) -> String {
    let body = body.unwrap_or("").trim_end();
    let escaped_signature = escape_html(signature).replace('\n', "<br>");
    if body.is_empty() {
        escaped_signature
    } else {
        format!("{body}<br><br>-- <br>{escaped_signature}")
    }
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn reply_subject(subject: &str) -> String {
    if subject.to_ascii_lowercase().starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {}", subject)
    }
}

pub fn header_string(headers: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    header_values(headers, key, false).into_iter().next()
}

pub fn header_references(headers: &BTreeMap<String, Value>) -> Vec<String> {
    header_values(headers, "references", true)
}

pub fn header_values(
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

pub fn value_to_strings(value: &Value, split_whitespace: bool) -> Vec<String> {
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

pub fn reply_headers_for_message(message: &MessageRecord) -> ReplyHeaders {
    let mut refs = message.references.clone();
    if let Some(message_id) = message.rfc_message_id.as_deref() {
        refs.push(message_id.to_string());
    }

    ReplyHeaders {
        in_reply_to: message.rfc_message_id.clone(),
        references: stable_dedup(refs),
    }
}

pub fn compact_targets(values: &[String]) -> String {
    if values.len() <= 2 {
        values.join(", ")
    } else {
        format!("{}, {} +{}", values[0], values[1], values.len() - 2)
    }
}

pub fn now_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn normalize_timestamp(value: Option<&str>) -> String {
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

pub fn has_short_numeric_offset(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    let sign = bytes[bytes.len() - 3];
    (sign == b'+' || sign == b'-')
        && bytes[bytes.len() - 2].is_ascii_digit()
        && bytes[bytes.len() - 1].is_ascii_digit()
}

pub fn stable_dedup(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

pub fn matching_account_email(
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

pub fn received_email_matches_account(email: &ReceivedEmail, account_email: &str) -> bool {
    let headers = email.headers.as_ref();
    let visible_to = headers
        .map(|headers| header_email_list(headers, "to"))
        .unwrap_or_default();
    let visible_cc = headers
        .map(|headers| header_email_list(headers, "cc"))
        .unwrap_or_default();
    let visible_bcc = headers
        .map(|headers| header_email_list(headers, "bcc"))
        .unwrap_or_default();

    matching_account_email(&email.to, &email.cc, &email.bcc, account_email)
        || matching_account_email(&visible_to, &visible_cc, &visible_bcc, account_email)
}

pub fn effective_received_to(email: &ReceivedEmail) -> Vec<String> {
    let header_to = email
        .headers
        .as_ref()
        .map(|headers| header_email_list(headers, "to"))
        .unwrap_or_default();
    if header_to.is_empty() {
        normalize_emails(&email.to)
    } else {
        header_to
    }
}

pub fn effective_received_cc(email: &ReceivedEmail) -> Vec<String> {
    let header_cc = email
        .headers
        .as_ref()
        .map(|headers| header_email_list(headers, "cc"))
        .unwrap_or_default();
    if header_cc.is_empty() {
        normalize_emails(&email.cc)
    } else {
        header_cc
    }
}

pub fn effective_received_bcc(email: &ReceivedEmail) -> Vec<String> {
    let header_bcc = email
        .headers
        .as_ref()
        .map(|headers| header_email_list(headers, "bcc"))
        .unwrap_or_default();
    if header_bcc.is_empty() {
        normalize_emails(&email.bcc)
    } else {
        header_bcc
    }
}

pub fn header_email_list(headers: &BTreeMap<String, Value>, key: &str) -> Vec<String> {
    header_values(headers, key, false)
        .into_iter()
        .flat_map(|value| split_address_header(&value))
        .map(|value| normalize_email(&value))
        .filter(|value| !value.is_empty())
        .collect()
}

pub fn split_address_header(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub fn ensure_reply_account_matches(message: &MessageRecord, account: &AccountRecord) -> Result<()> {
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

pub fn reply_recipients(message: &MessageRecord) -> Result<Vec<String>> {
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

pub fn sanitize_filename(name: &str, fallback: &str) -> String {
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

pub fn write_file_safely(dir: &Path, preferred_name: &str, bytes: &[u8]) -> Result<PathBuf> {
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

pub fn draft_attachment_root(base_dir: &Path) -> PathBuf {
    base_dir.join("draft-attachments")
}

pub fn snapshot_draft_attachments(
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

pub fn remove_draft_attachment_snapshot(base_dir: &Path, draft_id: &str) -> Result<()> {
    let snapshot_dir = draft_attachment_root(base_dir).join(draft_id);
    if snapshot_dir.exists() {
        fs::remove_dir_all(&snapshot_dir)
            .with_context(|| format!("failed to remove {}", snapshot_dir.display()))?;
    }
    Ok(())
}

pub fn build_idempotency_key(request: &SendEmailRequest) -> Result<String> {
    let _ = request;
    Ok(format!("email-cli-{}", uuid::Uuid::new_v4()))
}

#[allow(dead_code)]
pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
