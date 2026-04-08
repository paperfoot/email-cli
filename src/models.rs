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
    pub archived: bool,
}

/// Lightweight message for list/search/thread — no text_body/html_body to save tokens
#[derive(Debug, Serialize, Clone)]
pub struct MessageSummary {
    pub id: i64,
    pub remote_id: String,
    pub direction: String,
    pub account_email: String,
    pub from_addr: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub rfc_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub last_event: Option<String>,
    pub is_read: bool,
    pub created_at: String,
    pub archived: bool,
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

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
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

// ── Domain detail ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct DomainDetail {
    pub id: String,
    pub name: String,
    pub status: Option<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub records: Vec<DnsRecord>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DnsRecord {
    pub record: Option<String>,
    pub name: Option<String>,
    #[serde(alias = "type")]
    pub record_type: Option<String>,
    pub value: Option<String>,
    pub status: Option<String>,
    pub ttl: Option<String>,
    pub priority: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreateDomainRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateDomainResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct UpdateDomainRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_tracking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub click_tracking: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteResponse {
    #[serde(default)]
    pub deleted: bool,
}

/// Minimal `{ id }` response shape used by Resend's PATCH endpoints, which only echo
/// the resource id rather than returning the full updated resource.
#[derive(Debug, Deserialize, Serialize)]
pub struct IdResponse {
    pub id: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct SegmentRef {
    pub id: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct TopicRef {
    pub id: String,
    /// "opt_in" or "opt_out"
    pub subscription: String,
}

// ── Contact types ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Contact {
    pub id: String,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub unsubscribed: Option<bool>,
    pub created_at: Option<String>,
    /// Custom contact properties. Round-tripped from Resend's response when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Value>>,
}

#[derive(Debug, Serialize)]
pub struct CreateContactRequest {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsubscribed: Option<bool>,
    /// Free-form contact properties. Resend requires the property keys to be defined first
    /// via the contact-property schema CRUD (`email-cli contact-property create ...`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Value>>,
    /// Segments to add the contact to at create time. Each ref is `{id: "seg_..."}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<SegmentRef>>,
    /// Topics to subscribe the contact to at create time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<TopicRef>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateContactResponse {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct UpdateContactRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsubscribed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Value>>,
}

// ── Batch send ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct BatchSendResponse {
    #[serde(default)]
    pub data: Vec<BatchSendItem>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BatchSendItem {
    pub id: String,
}

// ── API key types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiKeyList {
    #[serde(default)]
    pub data: Vec<ApiKey>,
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateApiKeyResponse {
    pub id: String,
    pub token: String,
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

// ── Command log ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CommandLogEntry {
    pub id: i64,
    pub command: String,
    pub args: String,
    pub exit_code: Option<i32>,
    pub created_at: String,
}

// ── Segment types (Audiences renamed to Segments in November 2025) ────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Segment {
    pub id: String,
    pub name: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SegmentList {
    #[serde(default)]
    pub data: Vec<Segment>,
}

#[derive(Debug, Serialize)]
pub struct CreateSegmentRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateSegmentResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ContactSegmentResponse {
    pub id: String,
    #[serde(default)]
    pub deleted: bool,
}

// ── Broadcast types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Broadcast {
    pub id: String,
    pub name: Option<String>,
    /// Resend uses segment_id on the new API; the legacy field name was audience_id.
    /// Some endpoints/responses may still echo audience_id, so we accept both.
    #[serde(alias = "audience_id")]
    pub segment_id: Option<String>,
    pub from: Option<String>,
    pub subject: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    pub reply_to: Vec<String>,
    pub topic_id: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub preview_text: Option<String>,
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub scheduled_at: Option<String>,
    pub sent_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BroadcastList {
    #[serde(default)]
    pub data: Vec<Broadcast>,
}

#[derive(Debug, Serialize)]
pub struct CreateBroadcastRequest {
    pub segment_id: String,
    pub from: String,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<String>,
    /// If true, send the broadcast immediately after creation (single API call).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateBroadcastResponse {
    pub id: String,
}

#[derive(Debug, Serialize, Default)]
pub struct UpdateBroadcastRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct SendBroadcastRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SendBroadcastResponse {
    pub id: String,
}

// ── Contact property schema types ──────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContactProperty {
    pub id: String,
    pub key: String,
    #[serde(rename = "type")]
    pub property_type: String,
    pub fallback_value: Option<Value>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContactPropertyList {
    #[serde(default)]
    pub data: Vec<ContactProperty>,
}

#[derive(Debug, Serialize)]
pub struct CreateContactPropertyRequest {
    pub key: String,
    #[serde(rename = "type")]
    pub property_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_value: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateContactPropertyResponse {
    pub id: String,
}

#[derive(Debug, Serialize, Default)]
pub struct UpdateContactPropertyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_value: Option<Value>,
}

// ── Topic types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Topic {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub default_subscription: Option<String>,
    /// "public" or "private" — controls whether the topic is shown on the hosted preference page.
    pub visibility: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TopicList {
    #[serde(default)]
    pub data: Vec<Topic>,
}

#[derive(Debug, Serialize)]
pub struct CreateTopicRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// "opt_in" or "opt_out"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_subscription: Option<String>,
    /// "public" or "private"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateTopicResponse {
    pub id: String,
}

#[derive(Debug, Serialize, Default)]
pub struct UpdateTopicRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_subscription: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ContactTopicSubscription {
    pub id: String,
    /// "opt_in" or "opt_out"
    pub subscription: String,
}

#[derive(Debug, Serialize)]
pub struct UpdateContactTopicsRequest {
    pub topics: Vec<ContactTopicSubscription>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContactTopicView {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub subscription: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContactTopicList {
    #[serde(default)]
    pub data: Vec<ContactTopicView>,
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
