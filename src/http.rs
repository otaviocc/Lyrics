// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Lyrics-provider HTTP client, shared across every `ProviderKind`.
//!
//! Implements the politeness contract both known providers document: identify the client via
//! User-Agent, throttle requests, and honor `Retry-After` on 429. LRCLIB at
//! <https://lrclib.net/docs>, lrcmux at <https://lrcmux.dev/docs> (60 req/min, `Retry-After` on
//! 429, `User-Agent` recommended). Uses blocking `ureq` deliberately, since both contracts
//! require sequential requests, so async buys nothing.

use std::fmt::Write as _;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::meta::TrackMeta;
use crate::provider::{ProviderKind, ProviderSpec};

const DEFAULT_USER_AGENT: &str = concat!(
    "lyrics/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/otaviocc/Lyrics)"
);

/// A single lyrics record as returned by the LRCLIB-compatible API.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LyricsRecord {
    #[allow(dead_code)]
    pub id: i64,
    #[allow(dead_code)]
    pub track_name: String,
    #[allow(dead_code)]
    pub artist_name: String,
    pub album_name: Option<String>,
    pub duration: Option<f64>,
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

/// LRCLIB-style error envelope returned on non-retryable client errors.
#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[allow(dead_code)]
    code: i64,
    name: String,
    message: String,
}

/// Configuration for building an HTTP [`Client`].
pub struct ClientConfig {
    pub provider: ProviderKind,
    pub user_agent: Option<String>,
    pub delay_ms: u64,
    pub max_retries: u32,
    pub verbosity: u8,
}

/// Blocking HTTP client that talks to a lyrics provider.
///
/// Handles throttling, User-Agent identification, and automatic retry with exponential
/// backoff on 429 and 5xx responses. Not async; the provider contracts require sequential
/// requests, so async would add complexity for no benefit.
pub struct Client {
    agent: ureq::Agent,
    spec: ProviderSpec,
    user_agent: String,
    delay: Duration,
    max_retries: u32,
    last_request: Option<Instant>,
    verbosity: u8,
}

impl Client {
    /// Create a new client from the given configuration.
    #[must_use]
    pub fn new(config: ClientConfig) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into();

        Self {
            agent,
            spec: config.provider.spec(),
            user_agent: config
                .user_agent
                .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
            delay: Duration::from_millis(config.delay_ms),
            max_retries: config.max_retries,
            last_request: None,
            verbosity: config.verbosity,
        }
    }

    /// Sleep, if needed, so at least `self.delay` has elapsed since the previous request.
    /// Centralized here so no call site can bypass the throttle.
    fn throttle(&mut self) {
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if let Some(remaining) = self.delay.checked_sub(elapsed) {
                thread::sleep(remaining);
            }
        }
        self.last_request = Some(Instant::now());
    }

    fn log_request(&self, url: &str) {
        if self.verbosity >= 2 {
            eprintln!("[http] GET {url}");
        }
    }

    /// Issue a GET request with the throttle, User-Agent, and 429/5xx retry policy applied.
    /// Returns `Ok(None)` for a 404 (not an error, just "no such record").
    fn get_json<T: serde::de::DeserializeOwned>(&mut self, url: &str) -> Result<Option<T>> {
        let mut attempt = 0u32;
        loop {
            self.throttle();
            self.log_request(url);

            let mut request = self
                .agent
                .get(url)
                .header("User-Agent", &self.user_agent)
                .header("X-User-Agent", &self.user_agent);
            if let Some(header_name) = self.spec.client_header {
                request = request.header(header_name, &self.user_agent);
            }
            let result = request.call();

            let mut response = match result {
                Ok(response) => response,
                Err(err) => {
                    attempt = attempt.saturating_add(1);
                    if attempt > self.max_retries {
                        return Err(err).with_context(|| {
                            format!("{} request failed (transport error)", self.spec.name)
                        });
                    }
                    let wait = Duration::from_secs(2u64.saturating_pow(attempt));
                    thread::sleep(wait);
                    continue;
                }
            };

            let status = response.status();
            if status == 404 {
                return Ok(None);
            }
            if status.is_success() {
                let body: T = response
                    .body_mut()
                    .read_json()
                    .with_context(|| format!("failed to parse {} response body", self.spec.name))?;
                return Ok(Some(body));
            }

            attempt = attempt.saturating_add(1);
            if status == 429 {
                let wait = retry_after(response.headers())
                    .unwrap_or_else(|| Duration::from_secs(2u64.saturating_pow(attempt)));
                if attempt > self.max_retries {
                    bail!(
                        "{} rate limit exceeded (429) and max retries reached \
                         (server asked to wait {wait:?})",
                        self.spec.name
                    );
                }
                if self.verbosity >= 2 {
                    eprintln!("[http] 429 received, honoring Retry-After: {wait:?}");
                }
                thread::sleep(wait);
                continue;
            }
            if status.is_server_error() {
                if attempt > self.max_retries {
                    bail!(
                        "{} server error ({status}) and max retries reached",
                        self.spec.name
                    );
                }
                let wait = Duration::from_secs(2u64.saturating_pow(attempt));
                if self.verbosity >= 2 {
                    eprintln!("[http] {status} received, retrying in {wait:?}");
                }
                thread::sleep(wait);
                continue;
            }

            let message = response
                .body_mut()
                .read_json::<ErrorBody>()
                .map_or_else(|_| format!("HTTP {status}"), |b| describe_error(&b));
            bail!("{} request failed: {message}", self.spec.name);
        }
    }

    /// Look up a track by title, artist, and (optionally) album and duration.
    ///
    /// Returns `Ok(None)` when the provider has no matching record (HTTP 404).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, non-retryable HTTP errors, or when the provider's
    /// rate limit or server-error budget is exhausted.
    pub fn get(&mut self, meta: &TrackMeta) -> Result<Option<LyricsRecord>> {
        let mut url = format!(
            "{}?track_name={}&artist_name={}",
            self.spec.get_url,
            urlencode(&meta.title),
            urlencode(&meta.artist),
        );
        if let Some(album) = &meta.album {
            let _ = write!(url, "&album_name={}", urlencode(album));
        }
        if let Some(duration) = meta.duration {
            let _ = write!(url, "&duration={duration}");
        }
        self.get_json(&url)
    }

    /// Deliberately does **not** send `album_name`, even though `meta.album` may be known.
    /// LRCLIB filters `/api/search` results by `album_name` server-side, just as strictly as
    /// `/api/get` does, so passing it here would make the "loose" fallback fail in lockstep
    /// with the exact lookup on anything LRCLIB stores under a different album title (a
    /// remaster, reissue, or regional release). `pick_best_candidate` already scores album
    /// match as a client-side tiebreaker, which gets the same benefit without the false
    /// negatives.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, non-retryable HTTP errors, or when the provider's
    /// rate limit or server-error budget is exhausted.
    pub fn search(&mut self, meta: &TrackMeta) -> Result<Vec<LyricsRecord>> {
        let url = format!(
            "{}?track_name={}&artist_name={}",
            self.spec.search_url,
            urlencode(&meta.title),
            urlencode(&meta.artist),
        );
        Ok(self.get_json(&url)?.unwrap_or_default())
    }
}

/// Parse a `Retry-After` header per RFC 9110.
///
/// Accepts either an integer number of seconds or an HTTP-date. Returns `None` when the
/// header is absent or unparsable, in which case the caller falls back to exponential
/// backoff.
fn retry_after(headers: &http::HeaderMap) -> Option<Duration> {
    let value = headers.get("Retry-After")?.to_str().ok()?;
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let target = httpdate::parse_http_date(value.trim()).ok()?;
    let now = std::time::SystemTime::now();
    target.duration_since(now).ok()
}

/// Percent-encode a string for use in a query parameter (application/x-www-form-urlencoded).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b));
            }
            b' ' => out.push('+'),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Pick the best `/api/search` candidate.
///
/// Rejects anything outside `tolerance` seconds of the local duration (when known), then
/// prefers synced-available, then closest duration, then a matching album name.
///
/// # Panics
///
/// Panics if `duration_delta` returns `NaN` for a candidate, which cannot happen because all
/// inputs are finite `f64` values derived from `u32` subtractions.
#[must_use]
#[allow(clippy::unwrap_used)] // Documented in `# Panics` above.
pub fn pick_best_candidate(
    candidates: &[LyricsRecord],
    local_duration: Option<u32>,
    local_album: Option<&str>,
    tolerance: u32,
) -> Option<LyricsRecord> {
    let mut survivors: Vec<&LyricsRecord> = candidates
        .iter()
        .filter(|c| match (local_duration, c.duration) {
            (Some(local), Some(remote)) => {
                (f64::from(local) - remote).abs() <= f64::from(tolerance)
            }
            _ => true,
        })
        .collect();

    survivors.sort_by(|a, b| {
        let synced_key = |c: &&LyricsRecord| c.synced_lyrics.is_none();
        let duration_delta = |c: &&LyricsRecord| match (local_duration, c.duration) {
            (Some(local), Some(remote)) => (f64::from(local) - remote).abs(),
            _ => f64::MAX,
        };
        let album_key = |c: &&LyricsRecord| match (local_album, &c.album_name) {
            (Some(local), Some(remote)) => local.eq_ignore_ascii_case(remote),
            _ => false,
        };

        synced_key(a)
            .cmp(&synced_key(b))
            .then(duration_delta(a).partial_cmp(&duration_delta(b)).unwrap())
            .then(album_key(b).cmp(&album_key(a)))
    });

    survivors.first().map(|c| (*c).clone())
}

/// Format an LRCLIB error body for display in log output.
fn describe_error(body: &ErrorBody) -> String {
    format!("{} ({}): {}", body.name, body.code, body.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(duration: Option<f64>, synced: bool, album: Option<&str>) -> LyricsRecord {
        LyricsRecord {
            id: 1,
            track_name: "T".into(),
            artist_name: "A".into(),
            album_name: album.map(std::string::ToString::to_string),
            duration,
            instrumental: false,
            plain_lyrics: Some("plain".into()),
            synced_lyrics: synced.then(|| "[00:01.00]synced".into()),
        }
    }

    #[test]
    fn rejects_out_of_tolerance_candidates() {
        let candidates = vec![record(Some(300.0), false, None)];
        let best = pick_best_candidate(&candidates, Some(200), None, 2);
        assert!(best.is_none());
    }

    #[test]
    fn prefers_synced_over_closer_duration_plain() {
        let candidates = vec![
            record(Some(200.0), false, None), // exact duration match, plain only
            record(Some(201.0), true, None),  // 1s off, synced
        ];
        let best = pick_best_candidate(&candidates, Some(200), None, 2).unwrap();
        assert!(best.synced_lyrics.is_some());
    }

    #[test]
    fn tolerance_skipped_when_local_duration_unknown() {
        let candidates = vec![record(Some(9999.0), false, None)];
        let best = pick_best_candidate(&candidates, None, None, 2);
        assert!(best.is_some());
    }

    #[test]
    fn urlencode_handles_spaces_and_specials() {
        assert_eq!(urlencode("Borislav Slavov"), "Borislav+Slavov");
        assert_eq!(urlencode("Baldur's Gate"), "Baldur%27s+Gate");
    }

    #[test]
    fn retry_after_parses_integer_seconds() {
        let mut headers = http::HeaderMap::new();
        headers.insert("Retry-After", http::HeaderValue::from_static("2"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(2)));
    }

    #[test]
    fn retry_after_absent_returns_none() {
        let headers = http::HeaderMap::new();
        assert_eq!(retry_after(&headers), None);
    }
}
