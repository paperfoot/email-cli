use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::thread::sleep;
use std::time::Duration;

use crate::http::{backoff, retry_delay, should_retry_error, decode_json, decode_bytes};
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
        self.get_json(&format!("/emails/{}", id), &[])
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
        self.get_json(&format!("/emails/receiving/{}", id), &[])
    }

    pub fn list_received_attachments(&self, email_id: &str) -> Result<Vec<ReceivedAttachment>> {
        let payload: ListResponse<ReceivedAttachment> =
            self.get_json(&format!("/emails/receiving/{}/attachments", email_id), &[])?;
        Ok(payload.data)
    }

    // Domains
    pub fn get_domain(&self, id: &str) -> Result<DomainDetail> {
        self.get_json(&format!("/domains/{}", id), &[])
    }

    pub fn create_domain(&self, payload: &CreateDomainRequest) -> Result<CreateDomainResponse> {
        self.post_json("/domains", payload, None)
    }

    pub fn verify_domain(&self, id: &str) -> Result<DomainDetail> {
        self.post_json(&format!("/domains/{}/verify", id), &serde_json::json!({}), None)
    }

    pub fn delete_domain(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/domains/{}", id))
    }

    pub fn update_domain(&self, id: &str, payload: &UpdateDomainRequest) -> Result<DomainDetail> {
        self.patch_json(&format!("/domains/{}", id), payload)
    }

    // Audiences
    pub fn list_audiences(&self) -> Result<AudienceList> {
        self.get_json("/audiences", &[])
    }

    pub fn get_audience(&self, id: &str) -> Result<Audience> {
        self.get_json(&format!("/audiences/{}", id), &[])
    }

    pub fn create_audience(&self, payload: &CreateAudienceRequest) -> Result<CreateAudienceResponse> {
        self.post_json("/audiences", payload, None)
    }

    pub fn delete_audience(&self, id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/audiences/{}", id))
    }

    // Contacts
    pub fn list_contacts(&self, audience_id: &str) -> Result<ContactList> {
        self.get_json(&format!("/audiences/{}/contacts", audience_id), &[])
    }

    pub fn get_contact(&self, audience_id: &str, contact_id: &str) -> Result<Contact> {
        self.get_json(&format!("/audiences/{}/contacts/{}", audience_id, contact_id), &[])
    }

    pub fn create_contact(&self, audience_id: &str, payload: &CreateContactRequest) -> Result<CreateContactResponse> {
        self.post_json(&format!("/audiences/{}/contacts", audience_id), payload, None)
    }

    pub fn update_contact(&self, audience_id: &str, contact_id: &str, payload: &UpdateContactRequest) -> Result<Contact> {
        self.patch_json(&format!("/audiences/{}/contacts/{}", audience_id, contact_id), payload)
    }

    pub fn delete_contact(&self, audience_id: &str, contact_id: &str) -> Result<DeleteResponse> {
        self.delete_request(&format!("/audiences/{}/contacts/{}", audience_id, contact_id))
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
        self.delete_request(&format!("/api-keys/{}", id))
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
