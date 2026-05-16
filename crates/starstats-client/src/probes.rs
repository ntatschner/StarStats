//! Synchronous opt-in probes for user-entered configuration.
//!
//! `check_api_url` and `check_rsi_cookie` are *not* polled — they
//! fire on user click from the Settings pane. Each performs a single
//! HTTPS request with a tight timeout and returns a structured
//! result the UI can render inline next to the input that produced
//! it. Neither persists state; they're pure probes.

use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct ApiUrlCheck {
    pub ok: bool,
    pub status: Option<u16>,
    pub server_version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CookieCheck {
    pub ok: bool,
    pub handle: Option<String>,
    pub error: Option<String>,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Probe the configured StarStats server. `/healthz` is exposed by
/// `crates/starstats-server/src/main.rs:371`; we GET it with a 5s
/// timeout. Success means the URL resolves AND a StarStats server is
/// listening (HTTP 200). Server version is read from the optional
/// `X-Server-Version` header if present.
pub async fn check_api_url(url: String) -> ApiUrlCheck {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return ApiUrlCheck {
            ok: false,
            status: None,
            server_version: None,
            error: Some("Invalid URL — must start with http:// or https://".into()),
        };
    }
    let probe_url = format!("{}/healthz", url.trim_end_matches('/'));
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            return ApiUrlCheck {
                ok: false,
                status: None,
                server_version: None,
                error: Some(format!("Couldn't build HTTP client: {e}")),
            };
        }
    };
    match client.get(&probe_url).send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            let server_version = r
                .headers()
                .get("X-Server-Version")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            if r.status().is_success() {
                ApiUrlCheck {
                    ok: true,
                    status: Some(status),
                    server_version,
                    error: None,
                }
            } else {
                ApiUrlCheck {
                    ok: false,
                    status: Some(status),
                    server_version,
                    error: Some(format!("Server returned HTTP {status}")),
                }
            }
        }
        Err(e) => {
            let kind = if e.is_timeout() {
                "Timeout — server didn't respond in 5s"
            } else if e.is_connect() {
                "Couldn't connect — check the URL and your network"
            } else {
                "Network error"
            };
            ApiUrlCheck {
                ok: false,
                status: None,
                server_version: None,
                error: Some(format!("{kind}: {e}")),
            }
        }
    }
}

/// Probe an RSI session cookie by issuing one authenticated request
/// against `robertsspaceindustries.com`. Returns `ok: true` if RSI
/// accepts the cookie (HTTP 200 on the pledges page). Does NOT
/// persist the cookie — the user must explicitly hit Save.
///
/// The `handle` field is left `None` in this first pass — extracting
/// it reliably from the pledges page would require additional
/// scraping work; the validation signal alone ("the cookie is
/// accepted") is the load-bearing UX win.
pub async fn check_rsi_cookie(cookie: String) -> CookieCheck {
    let cookie = cookie.trim().to_string();
    if cookie.is_empty() {
        return CookieCheck {
            ok: false,
            handle: None,
            error: Some("Paste your Rsi-Token cookie first".into()),
        };
    }
    match crate::hangar::probe_with_cookie(&cookie).await {
        Ok(()) => CookieCheck {
            ok: true,
            handle: None,
            error: None,
        },
        Err(e) => CookieCheck {
            ok: false,
            handle: None,
            error: Some(format!("{e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_api_url_rejects_non_http() {
        let r = check_api_url("ftp://example.com".into()).await;
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn check_api_url_rejects_garbage() {
        let r = check_api_url("not even a url".into()).await;
        assert!(!r.ok);
    }

    #[tokio::test]
    async fn check_rsi_cookie_rejects_empty() {
        let r = check_rsi_cookie("".into()).await;
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("Paste"));
    }
}
