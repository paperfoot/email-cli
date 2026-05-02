use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::thread::sleep;
use std::time::Duration;

use crate::http::{backoff, decode_bytes, decode_json, retry_delay, should_retry_error};
use crate::models::*;

pub struct ResendClient {
    client: Client,
    api_key: String,
}

impl ResendClient {
    pub fn new(api_key: String) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent("email-cli/0.1.0")
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()
                .context("failed to build http client")?,
            api_key,
        })
    }

    pub fn list_domains(&self) -> Result<DomainList> {
        self.get_json("/domains", &[])
    }

    pub fn send_email(
        &self,
        payload: &SendEmailRequest,
        idempotency_key: &str,
    ) -> Result<SendEmailResponse> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Idempotency-Key",
            HeaderValue::from_str(idempotency_key).context("invalid idempotency key")?,
        );
        self.post_json("/emails", payload, Some(headers))
    }

    pub fn list_sent_emails_page(
        &self,
        limit: usize,
        after: Option<&str>,
    ) -> Result<ListResponse<SentEmail>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        self.get_json("/emails", &query)
    }

    pub fn get_sent_email(&self, id: &str) -> Result<SentEmail> {
        self.get_json(&format!("/emails/{}", urlencoding::encode(id)), &[])
    }

    pub fn list_received_emails_page(
        &self,
        limit: usize,
        after: Option<&str>,
    ) -> Result<ListResponse<ReceivedEmail>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        self.get_json("/emails/receiving", &query)
    }

    pub fn get_received_email(&self, id: &str) -> Result<ReceivedEmail> {
        self.get_json(
            &format!("/emails/receiving/{}", urlencoding::encode(id)),
            &[],
        )
    }

    pub fn list_received_attachments(&self, email_id: &str) -> Result<Vec<ReceivedAttachment>> {
        let payload: ListResponse<ReceivedAttachment> = self.get_json(
            &format!(
                "/emails/receiving/{}/attachments",
                urlencoding::encode(email_id)
            ),
            &[],
        )?;
        Ok(payload.data)
    }

    pub fn list_sent_attachments(&self, email_id: &str) -> Result<Vec<ReceivedAttachment>> {
        let payload: ListResponse<ReceivedAttachment> = self.get_json(
            &format!("/emails/{}/attachments", urlencoding::encode(email_id)),
            &[],
        )?;
        Ok(payload.data)
    }

    // Domains
    pub fn get_domain(&self, id: &str) -> Result<DomainDetail> {
        self.get_json(&format!("/domains/{}", urlencoding::encode(id)), &[])
    }

    pub fn create_domain(&self, payload: &CreateDomainRequest) -> Result<CreateDomainResponse> {
        self.post_json("/domains", payload, None)
    }

    pub fn verify_domain(&self, id: &str) -> Result<DomainDetail> {
        self.post_json(
            &format!("/domains/{}/verify", urlencoding::encode(id)),
            &serde_json::json!({}),
            None,
        )
    }

    pub fn delete_domain(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/domains/{}", urlencoding::encode(id)))
    }

    pub fn update_domain(&self, id: &str, payload: &UpdateDomainRequest) -> Result<DomainDetail> {
        self.patch_json(&format!("/domains/{}", urlencoding::encode(id)), payload)
    }

    // Contacts (flat /contacts endpoints — Resend renamed Audiences to Segments
    // in November 2025; new code uses these flat paths and treats segment grouping
    // as a separate concern via the segments service.)
    pub fn list_contacts_page(
        &self,
        limit: usize,
        after: Option<&str>,
    ) -> Result<ListResponse<Contact>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        self.get_json("/contacts", &query)
    }

    pub fn get_contact(&self, contact_id_or_email: &str) -> Result<Contact> {
        self.get_json(
            &format!("/contacts/{}", urlencoding::encode(contact_id_or_email)),
            &[],
        )
    }

    pub fn create_contact(&self, payload: &CreateContactRequest) -> Result<CreateContactResponse> {
        self.post_json("/contacts", payload, None)
    }

    pub fn update_contact(
        &self,
        contact_id_or_email: &str,
        payload: &UpdateContactRequest,
    ) -> Result<IdResponse> {
        self.patch_json(
            &format!("/contacts/{}", urlencoding::encode(contact_id_or_email)),
            payload,
        )
    }

    pub fn delete_contact(&self, contact_id_or_email: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!(
            "/contacts/{}",
            urlencoding::encode(contact_id_or_email)
        ))
    }

    // Batch
    pub fn send_batch(&self, emails: &[serde_json::Value]) -> Result<BatchSendResponse> {
        self.post_json("/emails/batch", &emails, None)
    }

    // API Keys
    pub fn list_api_keys(&self) -> Result<ApiKeyList> {
        self.get_json("/api-keys", &[])
    }

    pub fn create_api_key(&self, payload: &CreateApiKeyRequest) -> Result<CreateApiKeyResponse> {
        self.post_json("/api-keys", payload, None)
    }

    pub fn delete_api_key(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/api-keys/{}", urlencoding::encode(id)))
    }

    // Segments (Audiences renamed to Segments in November 2025; /audiences endpoints
    // are still backward-compatible.)
    pub fn list_segments(&self) -> Result<SegmentList> {
        // Resend defaults to limit=20 when omitted; pass the max so list commands don't
        // silently truncate at 20 for common-sized data.
        self.get_json("/segments", &[("limit", "100".to_string())])
    }

    pub fn get_segment(&self, id: &str) -> Result<Segment> {
        self.get_json(&format!("/segments/{}", urlencoding::encode(id)), &[])
    }

    pub fn create_segment(&self, payload: &CreateSegmentRequest) -> Result<CreateSegmentResponse> {
        self.post_json("/segments", payload, None)
    }

    pub fn delete_segment(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/segments/{}", urlencoding::encode(id)))
    }

    pub fn add_contact_to_segment(
        &self,
        contact_id_or_email: &str,
        segment_id: &str,
    ) -> Result<ContactSegmentResponse> {
        self.post_json(
            &format!(
                "/contacts/{}/segments/{}",
                urlencoding::encode(contact_id_or_email),
                urlencoding::encode(segment_id)
            ),
            &serde_json::json!({}),
            None,
        )
    }

    pub fn remove_contact_from_segment(
        &self,
        contact_id_or_email: &str,
        segment_id: &str,
    ) -> Result<ContactSegmentResponse> {
        self.delete_json(&format!(
            "/contacts/{}/segments/{}",
            urlencoding::encode(contact_id_or_email),
            urlencoding::encode(segment_id)
        ))
    }

    pub fn list_contact_segments(&self, contact_id_or_email: &str) -> Result<SegmentList> {
        self.get_json(
            &format!(
                "/contacts/{}/segments",
                urlencoding::encode(contact_id_or_email)
            ),
            &[("limit", "100".to_string())],
        )
    }

    pub fn list_segment_contacts(&self, segment_id: &str) -> Result<ListResponse<Contact>> {
        self.get_json(
            &format!("/segments/{}/contacts", urlencoding::encode(segment_id)),
            &[("limit", "100".to_string())],
        )
    }

    // Broadcasts
    pub fn list_broadcasts(&self) -> Result<BroadcastList> {
        self.get_json("/broadcasts", &[("limit", "100".to_string())])
    }

    pub fn get_broadcast(&self, id: &str) -> Result<Broadcast> {
        self.get_json(&format!("/broadcasts/{}", urlencoding::encode(id)), &[])
    }

    pub fn create_broadcast(
        &self,
        payload: &CreateBroadcastRequest,
    ) -> Result<CreateBroadcastResponse> {
        self.post_json("/broadcasts", payload, None)
    }

    pub fn update_broadcast(
        &self,
        id: &str,
        payload: &UpdateBroadcastRequest,
    ) -> Result<IdResponse> {
        self.patch_json(&format!("/broadcasts/{}", urlencoding::encode(id)), payload)
    }

    pub fn send_broadcast(
        &self,
        id: &str,
        payload: &SendBroadcastRequest,
    ) -> Result<SendBroadcastResponse> {
        self.post_json(
            &format!("/broadcasts/{}/send", urlencoding::encode(id)),
            payload,
            None,
        )
    }

    pub fn delete_broadcast(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/broadcasts/{}", urlencoding::encode(id)))
    }

    // Contact Properties (schema CRUD)
    pub fn list_contact_properties(&self) -> Result<ContactPropertyList> {
        self.get_json("/contact-properties", &[("limit", "100".to_string())])
    }

    pub fn get_contact_property(&self, id: &str) -> Result<ContactProperty> {
        self.get_json(
            &format!("/contact-properties/{}", urlencoding::encode(id)),
            &[],
        )
    }

    pub fn create_contact_property(
        &self,
        payload: &CreateContactPropertyRequest,
    ) -> Result<CreateContactPropertyResponse> {
        self.post_json("/contact-properties", payload, None)
    }

    pub fn update_contact_property(
        &self,
        id: &str,
        payload: &UpdateContactPropertyRequest,
    ) -> Result<IdResponse> {
        self.patch_json(
            &format!("/contact-properties/{}", urlencoding::encode(id)),
            payload,
        )
    }

    pub fn delete_contact_property(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/contact-properties/{}", urlencoding::encode(id)))
    }

    // Topics
    pub fn list_topics(&self) -> Result<TopicList> {
        self.get_json("/topics", &[("limit", "100".to_string())])
    }

    pub fn get_topic(&self, id: &str) -> Result<Topic> {
        self.get_json(&format!("/topics/{}", urlencoding::encode(id)), &[])
    }

    pub fn create_topic(&self, payload: &CreateTopicRequest) -> Result<CreateTopicResponse> {
        self.post_json("/topics", payload, None)
    }

    pub fn update_topic(&self, id: &str, payload: &UpdateTopicRequest) -> Result<IdResponse> {
        self.patch_json(&format!("/topics/{}", urlencoding::encode(id)), payload)
    }

    pub fn delete_topic(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/topics/{}", urlencoding::encode(id)))
    }

    pub fn update_contact_topics(
        &self,
        contact_id_or_email: &str,
        payload: &UpdateContactTopicsRequest,
    ) -> Result<serde_json::Value> {
        self.patch_json(
            &format!(
                "/contacts/{}/topics",
                urlencoding::encode(contact_id_or_email)
            ),
            payload,
        )
    }

    pub fn list_contact_topics(&self, contact_id_or_email: &str) -> Result<ContactTopicList> {
        self.get_json(
            &format!(
                "/contacts/{}/topics",
                urlencoding::encode(contact_id_or_email)
            ),
            &[("limit", "100".to_string())],
        )
    }

    pub fn download_attachment(&self, url: &str) -> Result<Vec<u8>> {
        for attempt in 0..5 {
            let response = match self.client.get(url).send() {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).context("attachment download failed"),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_bytes(response);
        }
        bail!("attachment download kept rate limiting")
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        for attempt in 0..5 {
            let response = match self
                .client
                .get(format!("https://api.resend.com{}", path))
                .bearer_auth(&self.api_key)
                .query(query)
                .send()
            {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).with_context(|| format!("GET {} failed", path)),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_json(response);
        }
        bail!("Resend API kept rate limiting for {}", path)
    }

    fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
        headers: Option<HeaderMap>,
    ) -> Result<T> {
        for attempt in 0..5 {
            let mut request = self
                .client
                .post(format!("https://api.resend.com{}", path))
                .bearer_auth(&self.api_key)
                .json(body);
            if let Some(extra_headers) = headers.clone() {
                request = request.headers(extra_headers);
            }
            let response = match request.send() {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).with_context(|| format!("POST {} failed", path)),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_json(response);
        }
        bail!("Resend API kept rate limiting for {}", path)
    }

    fn delete_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        for attempt in 0..5 {
            let response = match self
                .client
                .delete(format!("https://api.resend.com{}", path))
                .bearer_auth(&self.api_key)
                .send()
            {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).with_context(|| format!("DELETE {} failed", path)),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_json(response);
        }
        bail!("Resend API kept rate limiting for {}", path)
    }

    fn delete_request(&self, path: &str) -> Result<DeleteResponse> {
        for attempt in 0..5 {
            let response = match self
                .client
                .delete(format!("https://api.resend.com{}", path))
                .bearer_auth(&self.api_key)
                .send()
            {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).with_context(|| format!("DELETE {} failed", path)),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_json(response);
        }
        bail!("Resend API kept rate limiting for {}", path)
    }

    fn patch_json<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        for attempt in 0..5 {
            let response = match self
                .client
                .patch(format!("https://api.resend.com{}", path))
                .bearer_auth(&self.api_key)
                .json(body)
                .send()
            {
                Ok(response) => response,
                Err(err) if should_retry_error(&err) => {
                    sleep(backoff(attempt));
                    continue;
                }
                Err(err) => return Err(err).with_context(|| format!("PATCH {} failed", path)),
            };
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                sleep(retry_delay(response.headers(), attempt));
                continue;
            }
            if response.status().is_server_error() {
                sleep(backoff(attempt));
                continue;
            }
            return decode_json(response);
        }
        bail!("Resend API kept rate limiting for {}", path)
    }
}
