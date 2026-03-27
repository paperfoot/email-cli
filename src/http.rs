use anyhow::{Context, Result, bail};
use reqwest::blocking::Response;
use reqwest::header::HeaderMap;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::thread::sleep;
use std::time::Duration;

pub fn backoff(attempt: usize) -> Duration {
    let millis = 700_u64.saturating_mul((attempt as u64) + 1);
    Duration::from_millis(millis)
}

pub fn retry_delay(headers: &HeaderMap, attempt: usize) -> Duration {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| backoff(attempt))
}

pub fn should_retry_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

pub fn decode_json<T: DeserializeOwned>(response: Response) -> Result<T> {
    let status = response.status();
    let text = response.text().context("failed to read http response")?;
    if !status.is_success() {
        bail!("Resend API {}: {}", status, extract_error_message(&text));
    }
    serde_json::from_str(&text).context("failed to decode json response")
}

pub fn decode_bytes(response: Response) -> Result<Vec<u8>> {
    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        bail!(
            "download failed {}: {}",
            status,
            extract_error_message(&text)
        );
    }
    response
        .bytes()
        .map(|body| body.to_vec())
        .context("failed to read body")
}

pub fn extract_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(|message| message.as_str())
                .map(|message| message.to_string())
        })
        .unwrap_or_else(|| body.to_string())
}

pub fn fetch_sent_detail(client: &crate::resend::ResendClient, id: &str) -> Option<crate::models::SentEmail> {
    for attempt in 0..3 {
        if let Ok(detail) = client.get_sent_email(id) {
            return Some(detail);
        }
        sleep(Duration::from_millis(300 * ((attempt as u64) + 1)));
    }
    None
}
