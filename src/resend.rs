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
}
