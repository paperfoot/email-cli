use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

// ── Local records ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ProfileRecord {
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AccountRecord {
    pub email: String,
    pub profile_name: String,
    pub display_name: Option<String>,
    pub signature: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MessageRecord {
    pub id: i64,
    pub remote_id: String,
    pub direction: String,
    pub account_email: String,
    pub from_addr: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub reply_to: Vec<String>,
    pub subject: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub rfc_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub last_event: Option<String>,
    pub is_read: bool,
    pub created_at: String,
    pub synced_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct DraftRecord {
    pub id: String,
    pub account_email: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub reply_to_message_id: Option<i64>,
    pub attachment_paths: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AttachmentRecord {
    pub id: i64,
    pub message_id: i64,
    pub remote_attachment_id: Option<String>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: Option<i64>,
    pub download_url: Option<String>,
    pub local_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncSummary {
    pub profiles: usize,
    pub sent_messages: usize,
    pub received_messages: usize,
}

// ── Resend API types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct DomainList {
    #[serde(default)]
    pub data: Vec<Domain>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Domain {
    pub name: String,
    pub status: Option<String>,
    pub region: Option<String>,
    pub capabilities: Option<DomainCapabilities>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DomainCapabilities {
    pub sending: Option<String>,
    pub receiving: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendEmailRequest {
    pub from: String,
    pub to: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bcc: Vec<String>,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<SendAttachment>,
}

#[derive(Debug, Serialize)]
pub struct SendAttachment {
    pub filename: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct SendEmailResponse {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ListResponse<T> {
    #[serde(default)]
    pub data: Vec<T>,
    pub has_more: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct SentEmail {
    pub id: String,
    pub from: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub to: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub cc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub bcc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub reply_to: Vec<String>,
    pub subject: Option<String>,
    pub created_at: Option<String>,
    pub last_event: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ReceivedEmail {
    pub id: String,
    pub from: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub to: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub cc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub bcc: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub reply_to: Vec<String>,
    pub subject: Option<String>,
    pub created_at: Option<String>,
    pub message_id: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub attachments: Vec<ReceivedAttachment>,
    pub headers: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ReceivedAttachment {
    pub id: Option<String>,
    pub filename: Option<String>,
    #[serde(alias = "contentType")]
    pub content_type: Option<String>,
    pub size: Option<i64>,
    #[serde(alias = "downloadUrl")]
    pub download_url: Option<String>,
}

// ── Internal types ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ResolvedCompose {
    pub account: AccountRecord,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct ReplyHeaders {
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

#[derive(Clone)]
pub struct MessageUpsert {
    pub remote_id: String,
    pub direction: String,
    pub account_email: String,
    pub from_addr: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub reply_to: Vec<String>,
    pub subject: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub rfc_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub last_event: Option<String>,
    pub is_read: bool,
    pub created_at: String,
    pub raw_json: String,
}

// ── Custom deserializer ────────────────────────────────────────────────────

pub fn deserialize_string_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|item| item.as_str().map(|value| value.to_string()))
            .collect()),
        Value::String(value) => Ok(vec![value]),
        Value::Null => Ok(Vec::new()),
        _ => Err(serde::de::Error::custom("expected string array or null")),
    }
}
