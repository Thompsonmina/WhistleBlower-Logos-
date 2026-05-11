use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{debug, warn};

use super::envelope::Envelope;

#[derive(Clone)]
pub struct DeliveryClient {
    base: String,
    http: reqwest::Client,
}

/// Raw message shape returned by GET /relay/v1/auto/messages/<topic>.
#[derive(Deserialize)]
struct WakuMessage {
    payload: String, // base64-encoded
    #[serde(rename = "contentTopic")]
    content_topic: String,
}

impl DeliveryClient {
    pub fn new(rest_url: impl Into<String>) -> Self {
        Self {
            base: rest_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Returns true when the nwaku REST API is reachable.
    pub async fn healthy(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Subscribe to `content_topic` via autosharding.
    /// Safe to call multiple times — nwaku silently deduplicates subscriptions.
    pub async fn subscribe(&self, content_topic: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/relay/v1/auto/subscriptions", self.base))
            .json(&serde_json::json!([content_topic]))
            .send()
            .await
            .context("POST /relay/v1/auto/subscriptions")?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        // nwaku returns 200 with body "OK" on success (some versions 204).
        if !status.is_success() && !body.trim().eq_ignore_ascii_case("ok") {
            anyhow::bail!("subscribe failed ({}): {}", status, body);
        }
        Ok(())
    }

    /// Drain the message queue for `content_topic`.
    ///
    /// nwaku clears its internal queue on each GET, so messages are consumed
    /// exactly once per call.  Returns only successfully parsed envelopes;
    /// malformed payloads are logged and skipped.
    pub async fn drain(&self, content_topic: &str) -> Result<Vec<Envelope>> {
        let encoded = urlencoding::encode(content_topic).into_owned();
        let url = format!("{}/relay/v1/auto/messages/{}", self.base, encoded);

        let resp = self.http.get(&url).send().await.context("GET messages")?;

        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        resp.error_for_status_ref().context("drain messages")?;

        let raw: Vec<WakuMessage> = resp.json().await.context("parse messages JSON")?;
        debug!("drained {} raw message(s)", raw.len());

        let envelopes = raw
            .iter()
            .filter_map(|msg| match Envelope::from_payload(&msg.payload) {
                Ok(env) => Some(env),
                Err(e) => {
                    warn!(
                        topic = %msg.content_topic,
                        "skipping malformed payload: {e}"
                    );
                    None
                }
            })
            .collect();

        Ok(envelopes)
    }

    /// Query nwaku's store-protocol REST endpoint for messages on `content_topic`
    /// stored at or after `start_time_ns` (Unix nanoseconds).
    ///
    /// Pages through results until the server returns no `paginationCursor`.
    /// Malformed payloads are logged and skipped.  Returns envelopes in
    /// storage order (oldest-first when nwaku is ascending).
    pub async fn query_store(
        &self,
        content_topic: &str,
        start_time_ns: u128,
    ) -> Result<Vec<Envelope>> {
        let mut cursor: Option<String> = None;
        let mut envelopes: Vec<Envelope> = Vec::new();
        let encoded_topic = urlencoding::encode(content_topic).into_owned();
        const PAGE_SIZE: u32 = 100;

        loop {
            let mut url = format!(
                "{}/store/v3/messages?contentTopics={}&startTime={}&pageSize={}&includeData=true&ascending=true",
                self.base, encoded_topic, start_time_ns, PAGE_SIZE,
            );
            if let Some(c) = &cursor {
                url.push_str("&cursor=");
                url.push_str(c);
            }

            let resp = self.http.get(&url).send().await.context("GET /store/v3/messages")?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("store query failed ({}): {}", status, body);
            }
            let page: StoreResponse = resp.json().await.context("parse store response")?;

            for entry in &page.messages {
                let Some(msg) = &entry.message else { continue };
                match Envelope::from_payload(&msg.payload) {
                    Ok(env) => envelopes.push(env),
                    Err(e) => warn!(
                        topic = %msg.content_topic,
                        "store: skipping malformed payload: {e}"
                    ),
                }
            }

            debug!(
                page_size = page.messages.len(),
                total = envelopes.len(),
                "store page fetched"
            );

            match page.pagination_cursor {
                Some(c) if !c.is_empty() && page.messages.len() == PAGE_SIZE as usize => {
                    cursor = Some(c);
                }
                _ => break,
            }
        }

        Ok(envelopes)
    }
}

/// Top-level shape of GET /store/v3/messages?includeData=true.
#[derive(Deserialize)]
struct StoreResponse {
    #[serde(default)]
    messages: Vec<StoreMessageEntry>,
    #[serde(rename = "paginationCursor", default)]
    pagination_cursor: Option<String>,
}

#[derive(Deserialize)]
struct StoreMessageEntry {
    #[serde(default)]
    message: Option<StoredWakuMessage>,
}

#[derive(Deserialize)]
struct StoredWakuMessage {
    payload: String,
    #[serde(rename = "contentTopic")]
    content_topic: String,
}
