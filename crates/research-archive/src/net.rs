//! The live transport: blocking HTTPS GETs with a timeout, a minimum gap between requests to one host, a bounded
//! retry on rate limiting and server errors, and an API key sent as a bearer header (never in the URL, where it
//! would reach logs and error messages; an OpenAlex key is also the key holder's sign-in credential).
//!
//! The client is reqwest's blocking one: call it from a plain thread, never from inside an async runtime.

use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use crate::index::{Error, Fetch};
use crate::openalex::{self, Budget};

/// The longest Retry-After a request will sit out. Longer means a spent quota: the run stops and says so.
const MAX_WAIT_S: u64 = 30;

pub struct HttpFetch {
    client: reqwest::blocking::Client,
    /// The least time between two requests to the same host. arXiv asks for three seconds.
    gap: HashMap<String, Duration>,
    last: HashMap<String, Instant>,
    /// Host -> key, sent as `Authorization: Bearer`.
    bearer: HashMap<String, String>,
    retries: u32,
    /// Requests actually sent, retries included.
    pub sent: usize,
}

impl HttpFetch {
    pub fn new() -> Result<HttpFetch, Error> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("alelyon-research/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| Error::Http(e.without_url().to_string()))?;
        let gap = [("export.arxiv.org".to_string(), Duration::from_secs(3)), ("api.openalex.org".to_string(), Duration::from_millis(120))].into();
        Ok(HttpFetch { client, gap, last: HashMap::new(), bearer: HashMap::new(), retries: 3, sent: 0 })
    }

    /// Send `key` to `host` (and only to it) as a bearer token. An empty key sends nothing.
    pub fn with_bearer(mut self, host: &str, key: Option<String>) -> Self {
        if let Some(k) = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
            self.bearer.insert(host.to_string(), k);
        }
        self
    }

    /// OpenAlex's account of the configured key's daily budget. With no key configured this reports the keyless
    /// budget shared by the network's address.
    pub fn openalex_budget(&mut self) -> Result<Budget, Error> {
        openalex::parse_budget(&self.get(openalex::RATE_LIMIT_URL)?)
    }

    fn wait_turn(&mut self, host: &str) {
        let gap = self.gap.get(host).copied().unwrap_or(Duration::from_millis(250));
        if let Some(t) = self.last.get(host) {
            let since = t.elapsed();
            if since < gap {
                thread::sleep(gap - since);
            }
        }
        self.last.insert(host.to_string(), Instant::now());
    }
}

impl Fetch for HttpFetch {
    fn get(&mut self, url: &str) -> Result<String, Error> {
        let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        let mut attempt = 0;
        loop {
            self.wait_turn(&host);
            self.sent += 1;
            let mut req = self.client.get(url);
            if let Some(k) = self.bearer.get(&host) {
                req = req.bearer_auth(k);
            }
            let resp = req.send().map_err(|e| Error::Http(format!("{host}: {}", e.without_url())))?;
            let status = resp.status();
            if status.is_success() {
                return resp.text().map_err(|e| Error::Http(format!("{host}: {}", e.without_url())));
            }
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(Error::KeyRefused(format!("{host} answered {status}")));
            }
            if status.as_u16() == 429 {
                let wait = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
                match wait {
                    Some(s) if s <= MAX_WAIT_S && attempt < self.retries => {
                        attempt += 1;
                        thread::sleep(Duration::from_secs(s.max(1)));
                        continue;
                    }
                    None if attempt < self.retries => {
                        attempt += 1;
                        thread::sleep(Duration::from_secs(2u64.pow(attempt)));
                        continue;
                    }
                    _ => {
                        // Only OpenAlex sells keys; another service throttling is told as it is.
                        let hint = match (host.as_str(), self.bearer.contains_key(&host)) {
                            ("api.openalex.org", true) => "; today's budget for this key is spent",
                            ("api.openalex.org", false) => "; the keyless budget is spent, add an API key",
                            _ => "; it is limiting how often it is asked",
                        };
                        return Err(Error::RateLimited(format!(
                            "{host} asks to wait {} s{hint}",
                            wait.map_or("an unknown number of".into(), |s| s.to_string())
                        )));
                    }
                }
            }
            if status.is_server_error() && attempt < self.retries {
                attempt += 1;
                thread::sleep(Duration::from_secs(2u64.pow(attempt)));
                continue;
            }
            return Err(Error::Http(format!("{host} answered {status}")));
        }
    }
}
