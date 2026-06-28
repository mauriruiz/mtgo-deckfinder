//! Polite blocking HTTP client: explicit User-Agent, minimum spacing between
//! requests, and retry-with-backoff on 429 / 5xx / transport errors.

use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

use reqwest::blocking::{Client, Response};
use reqwest::header::ACCEPT;

use crate::USER_AGENT;
use crate::error::{Error, Result};

const MAX_RETRIES: u32 = 3;

/// A blocking HTTP client that rate-limits itself to one request per
/// `min_interval` and retries transient failures.
pub struct PoliteClient {
    client: Client,
    min_interval: Duration,
    last_request: Mutex<Option<Instant>>,
}

impl PoliteClient {
    /// Build a client that waits at least `min_interval` between requests.
    pub fn new(min_interval: Duration) -> Result<Self> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(120))
            // mtgo.com's HTTP/2 endpoint hangs (verified); pin HTTP/1.1.
            .http1_only()
            .build()?;
        Ok(Self {
            client,
            min_interval,
            last_request: Mutex::new(None),
        })
    }

    /// GET and decode the body as UTF-8 text.
    pub fn get_text(&self, url: &str) -> Result<String> {
        Ok(self.get(url)?.text()?)
    }

    /// GET and return the raw body bytes.
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        Ok(self.get(url)?.bytes()?.to_vec())
    }

    fn throttle(&self) {
        let mut last = self.last_request.lock().expect("lock poisoned");
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < self.min_interval {
                sleep(self.min_interval - elapsed);
            }
        }
        *last = Some(Instant::now());
    }

    fn get(&self, url: &str) -> Result<Response> {
        // ponytail: fixed exponential backoff, no jitter — fine for a single-user
        // CLI; add jitter if this ever runs many concurrent clients.
        let mut backoff = Duration::from_secs(2);
        for attempt in 0..=MAX_RETRIES {
            self.throttle();
            match self.client.get(url).header(ACCEPT, "*/*").send() {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    // Retry rate-limits and server errors; fail fast on other 4xx.
                    if status.as_u16() != 429 && !status.is_server_error() {
                        return Err(Error::Parse(format!("HTTP {status} for {url}")));
                    }
                }
                Err(e) if attempt == MAX_RETRIES => return Err(e.into()),
                Err(_) => {}
            }
            if attempt < MAX_RETRIES {
                sleep(backoff);
                backoff *= 2;
            }
        }
        Err(Error::Parse(format!("exhausted retries for {url}")))
    }
}
