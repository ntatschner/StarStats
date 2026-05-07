//! Tray-side hangar fetcher.
//!
//! Periodically scrapes the user's RSI pledge ledger
//! (https://robertsspaceindustries.com/account/pledges) using the
//! session cookie stored in the OS keychain, parses the page into a
//! `Vec<HangarShip>`, and POSTs the snapshot to the StarStats server.
//!
//! The session cookie itself never leaves the user's machine — it is
//! read directly from `keyring`, sent only to RSI, and forgotten at
//! end of each fetch cycle. The server only ever sees the parsed,
//! structured ship list.
//!
//! EAC-aware: the worker consults [`crate::process_guard`] before
//! every fetch and skips the cycle if Star Citizen is running.
//! Authenticated HTTP from the same machine while the game is active
//! can trip Easy Anti-Cheat heuristics; a missed cycle is a much
//! cheaper failure mode than a banned account.

use crate::process_guard::is_starcitizen_running;
use crate::secret::{SecretStore, ACCOUNT_RSI_SESSION_COOKIE};
use crate::state::AccountStatus;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use reqwest::StatusCode;
use scraper::{Html, Selector};
use serde::Serialize;
use starstats_core::wire::{HangarPushRequest, HangarShip};
use std::sync::Arc;
use std::time::Duration;

/// One refresh cycle every 6 hours when idle. Hangar contents change
/// rarely (a pledge is bought / melted maybe once a week even for
/// active users), and we don't want to over-poll RSI's authenticated
/// endpoints — a single misbehaving client cohort would look like a
/// scraper to RSI's WAF.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// HTTP timeout for the RSI page fetch + the StarStats POST. RSI's
/// authenticated pages are slow (~3–5s observed); 30s leaves headroom.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum body size we'll accept from RSI's pledge page. Live pages
/// are ~200 KB; 5 MB is roomy enough for an account with hundreds of
/// pledges and bounds the parse cost. Mirror of the server-side
/// pattern in `rsi_verify::MAX_PROFILE_BODY_BYTES`.
pub const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;

/// Per-field cap matching the server's `hangar_routes::MAX_FIELD_CHARS`.
/// Kept identical so a long ship name is dropped client-side BEFORE
/// it ever reaches the server's validator (failing the entire push).
pub const MAX_FIELD_CHARS: usize = 200;

/// Hard cap on ships in a single push, mirroring server's
/// `hangar_routes::MAX_SHIPS_PER_PUSH`.
pub const MAX_SHIPS_PER_PUSH: usize = 5000;

/// RSI pledge ledger URL. Authenticated — requires a valid session
/// cookie attached as a request header.
pub const PLEDGES_URL: &str = "https://robertsspaceindustries.com/account/pledges";

/// Name of the RSI session cookie. The user pastes the **value** of
/// this cookie out of their browser's DevTools cookie store (not the
/// full `Cookie:` header — just the value). The tray reassembles the
/// header at fetch time as `Cookie: Rsi-Token=<value>`.
///
/// Capital-R, capital-T — RSI's frontend sets the cookie that exact
/// way; HTTP cookie names are case-sensitive in practice on RSI's
/// stack even though the spec says otherwise.
pub const RSI_SESSION_COOKIE_NAME: &str = "Rsi-Token";

/// User-Agent — same shape as the server's RSI scraper so RSI's WAF
/// sees a single coherent client identity across the project.
const USER_AGENT: &str = concat!(
    "StarStats/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/RSIStarCitizenTools/StarStats)"
);

/// Surfaced in the tray UI via Tauri commands (Worker E will wire it).
/// All timestamps are RFC3339; `last_skip_reason` is a short token
/// suitable for direct comparison in the React frontend (e.g.
/// `"game running"`, `"no cookie set"`, `"rsi_cookie_invalid"`).
#[derive(Debug, Serialize, Default, Clone)]
pub struct HangarStats {
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub ships_pushed: u32,
    pub last_skip_reason: Option<String>,
}

/// Spawn the hangar refresh worker. Returns the `JoinHandle` so the
/// caller can drop it with the runtime; the worker itself runs forever.
///
/// Note: this is intentionally NOT gated on `cfg.enabled` like
/// `sync::start` — Worker E decides at the call site whether to spawn
/// at all (no API URL / no token => no spawn). Once spawned, the
/// worker keeps running until the runtime drops; per-cycle decisions
/// (cookie present? game running?) are made inside `refresh_once`.
#[allow(dead_code)] // wired by Worker E
pub fn start(
    api_url: String,
    access_token: String,
    hangar_stats: Arc<Mutex<HangarStats>>,
    account_status: Arc<Mutex<AccountStatus>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let secret = match SecretStore::new(ACCOUNT_RSI_SESSION_COOKIE) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "hangar worker: keychain unavailable; not starting");
                return;
            }
        };

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "hangar worker: reqwest client build failed; not starting");
                return;
            }
        };

        // First cycle runs immediately so the user gets feedback on
        // the freshly-pasted cookie. Subsequent cycles wait the full
        // REFRESH_INTERVAL.
        loop {
            // Don't push to a server that has already rejected our
            // device token — the StarStats POST below would just 401
            // again. Hangar reads on RSI's side aren't gated by our
            // server's auth state, but the push is, so the whole cycle
            // is gated. Local-only fetch + parse without an upstream
            // push has no caller today.
            if !account_status.lock().auth_lost {
                if let Err(e) =
                    refresh_once(&client, &api_url, &access_token, &secret, &hangar_stats).await
                {
                    tracing::warn!(error = %e, "hangar refresh failed");
                    let mut s = hangar_stats.lock();
                    s.last_error = Some(e.to_string());
                }
            }
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    })
}

/// Build the reqwest client used for both the RSI fetch and the
/// StarStats POST. Cookie jar is enabled so any 30x dance RSI's WAF
/// runs (token-refresh redirect, region rewrite) preserves Set-Cookie
/// across hops; gzip is enabled because RSI's pledge page compresses
/// to ~10% of its raw size.
fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .cookie_store(true)
        .gzip(true)
        // Cap redirects — RSI sometimes loops between login and the
        // ledger when the cookie is dead. The cap stops a misconfigured
        // upstream from holding our connection.
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("build hangar http client")
}

#[allow(dead_code)] // wired by Worker E
async fn refresh_once(
    client: &reqwest::Client,
    api_url: &str,
    access_token: &str,
    secret: &SecretStore,
    hangar_stats: &Mutex<HangarStats>,
) -> Result<()> {
    {
        let mut s = hangar_stats.lock();
        s.last_attempt_at = Some(now_rfc3339());
    }

    if is_starcitizen_running() {
        tracing::debug!("hangar refresh skipped: Star Citizen is running");
        let mut s = hangar_stats.lock();
        s.last_skip_reason = Some("game running".into());
        return Ok(());
    }

    let cookie_value = match secret.get().context("read RSI cookie from keychain")? {
        Some(v) if !v.trim().is_empty() => v,
        _ => {
            tracing::debug!("hangar refresh skipped: no RSI cookie set");
            let mut s = hangar_stats.lock();
            s.last_skip_reason = Some("no cookie set".into());
            return Ok(());
        }
    };

    // Stamp clears the previous skip reason — this is a real attempt.
    {
        let mut s = hangar_stats.lock();
        s.last_skip_reason = None;
    }

    let body = match fetch_pledges(client, &cookie_value, hangar_stats).await? {
        Some(body) => body,
        // RSI rejected the cookie. State has already been recorded by
        // `fetch_pledges`; bail without erroring out so the loop sleeps.
        None => return Ok(()),
    };

    let parsed = parse_pledges_html(&body);
    let ships = sanitise_ships(parsed);
    tracing::info!(count = ships.len(), "hangar parsed");

    let push = HangarPushRequest {
        schema_version: 1,
        ships,
    };

    let url = format!("{}/v1/me/hangar", api_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&push)
        .send()
        .await
        .context("POST /v1/me/hangar")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("hangar push failed: {status} {body}");
    }

    let pushed = push.ships.len() as u32;
    let mut s = hangar_stats.lock();
    s.last_success_at = Some(now_rfc3339());
    s.last_error = None;
    s.ships_pushed = pushed;
    Ok(())
}

/// Fetch the RSI pledge ledger HTML using the supplied session cookie.
///
/// Returns `Ok(Some(body))` on a clean 200, `Ok(None)` if RSI rejected
/// the cookie (401/403 — recorded as a skip reason so the user knows
/// to re-paste), and `Err(_)` for transport / non-2xx errors.
async fn fetch_pledges(
    client: &reqwest::Client,
    cookie_value: &str,
    hangar_stats: &Mutex<HangarStats>,
) -> Result<Option<String>> {
    let cookie_header = format!("{}={}", RSI_SESSION_COOKIE_NAME, cookie_value);
    let resp = client
        .get(PLEDGES_URL)
        .header(reqwest::header::COOKIE, cookie_header)
        .send()
        .await
        .context("GET RSI pledges")?;

    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        tracing::warn!(%status, "RSI cookie expired or invalid — pausing until user re-pastes");
        let mut s = hangar_stats.lock();
        s.last_error = Some("RSI cookie expired or invalid".into());
        s.last_skip_reason = Some("rsi_cookie_invalid".into());
        return Ok(None);
    }
    if !status.is_success() {
        anyhow::bail!("RSI pledges returned {status}");
    }

    let body = read_capped_text(resp)
        .await
        .context("read RSI pledges body")?;
    Ok(Some(body))
}

/// Stream-read a response body into a `String`, aborting if it crosses
/// [`MAX_BODY_BYTES`]. Tray-side analogue of the server's
/// `rsi_verify::read_capped_text`. `reqwest::Response::text` has no
/// ceiling, so a misbehaving upstream could balloon the allocation.
async fn read_capped_text(mut resp: reqwest::Response) -> Result<String> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match resp.chunk().await.context("read chunk")? {
            Some(chunk) => {
                if buf.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
                    anyhow::bail!("RSI pledges body exceeded {MAX_BODY_BYTES}-byte cap; aborting");
                }
                buf.extend_from_slice(&chunk);
            }
            None => break,
        }
    }
    String::from_utf8(buf).context("RSI pledges body is not utf-8")
}

/// Drop ships whose any field exceeds [`MAX_FIELD_CHARS`] (rather than
/// truncating — the user is better served by a missing entry they can
/// hover-explain than by a silently-mangled name). Truncate the list
/// at [`MAX_SHIPS_PER_PUSH`] so a runaway parser never produces a
/// payload the server's validator would reject in its entirety.
fn sanitise_ships(ships: Vec<HangarShip>) -> Vec<HangarShip> {
    let mut out = Vec::with_capacity(ships.len().min(MAX_SHIPS_PER_PUSH));
    for ship in ships {
        if ship.name.trim().is_empty() {
            continue;
        }
        if field_too_long(&ship.name)
            || ship.manufacturer.as_deref().is_some_and(field_too_long)
            || ship.pledge_id.as_deref().is_some_and(field_too_long)
            || ship.kind.as_deref().is_some_and(field_too_long)
        {
            tracing::warn!(name = %ship.name, "dropping ship with oversize field");
            continue;
        }
        if out.len() >= MAX_SHIPS_PER_PUSH {
            tracing::warn!(
                cap = MAX_SHIPS_PER_PUSH,
                "ships truncated at server-side cap"
            );
            break;
        }
        out.push(ship);
    }
    out
}

fn field_too_long(s: &str) -> bool {
    s.chars().count() > MAX_FIELD_CHARS
}

// -- HTML parser ----------------------------------------------------
//
// RSI's pledges page is server-rendered HTML. The actual live markup
// will likely differ from the fixture below — the parser is
// deliberately defensive: every selector falls back to `None`, and
// only `name` is required to retain the entry. The fixture locks the
// CONTRACT the parser depends on, so when RSI reshuffles its markup
// the fix is a fixture update + selector update in lockstep — caught
// at CI time, not in production.
//
// Fixture-driven contract (see tests below):
//   * Each pledge sits inside a container matching `.pledge` (or
//     `article.pledge`).
//   * Stable pledge identifier: the container's `data-pledge-id`
//     attribute (RSI exposes one in their existing API surfaces).
//   * Display name: a descendant matching `.pledge-name` (fallback
//     `.js-pledge-name`).
//   * Manufacturer: `.pledge-manufacturer` (fallback `.manufacturer`).
//   * Kind / classification: `.pledge-kind` (fallback `.kind`).
//
// Real RSI HTML is more verbose; the parser only depends on the
// distinguishing class names and the data attribute, not on the wider
// DOM shape.

/// Parse a pledges page body into a list of [`HangarShip`].
///
/// Best-effort: missing fields collapse to `None`, malformed entries
/// (no name) are dropped. Returns an empty vec on garbage input
/// rather than panicking, matching the posture of `parse_orgs_html`
/// on the server.
pub fn parse_pledges_html(body: &str) -> Vec<HangarShip> {
    let doc = Html::parse_document(body);

    let Ok(pledge_sel) = Selector::parse(".pledge") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for el in doc.select(&pledge_sel) {
        let Some(parsed) = parse_pledge_block(&el) else {
            continue;
        };
        out.push(parsed);
    }
    out
}

fn parse_pledge_block(el: &scraper::ElementRef<'_>) -> Option<HangarShip> {
    let name = find_first_text(el, &[".pledge-name", ".js-pledge-name", ".name"])?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return None;
    }

    let manufacturer = find_first_text(el, &[".pledge-manufacturer", ".manufacturer"])
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let kind = find_first_text(el, &[".pledge-kind", ".kind"])
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let pledge_id = el
        .value()
        .attr("data-pledge-id")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    Some(HangarShip {
        name,
        manufacturer,
        pledge_id,
        kind,
    })
}

/// Walk the candidate selectors in order and return the trimmed text
/// of the first matching descendant. Same pattern as the server's
/// `rsi_verify::find_first_text` — duplicated rather than depended on
/// to keep the tray's compile graph independent of the server crate.
fn find_first_text(el: &scraper::ElementRef<'_>, selectors: &[&str]) -> Option<String> {
    for raw in selectors {
        let Ok(sel) = Selector::parse(raw) else {
            continue;
        };
        if let Some(child) = el.select(&sel).next() {
            return Some(collect_text(&child));
        }
    }
    None
}

/// Concatenate all descendant text inside an element, preserving a
/// single space between adjacent nodes.
fn collect_text(el: &scraper::ElementRef<'_>) -> String {
    let mut out = String::new();
    for chunk in el.text() {
        if !out.is_empty()
            && !out.ends_with(char::is_whitespace)
            && !chunk.starts_with(char::is_whitespace)
        {
            out.push(' ');
        }
        out.push_str(chunk);
    }
    out
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stripped-down version of what we expect the live pledges page
    /// to look like. Locks the parser's contract — when RSI reshuffles
    /// markup, update this fixture + the selectors in lockstep.
    const FULL_FIXTURE: &str = r#"
        <html><body>
        <div class="pledge-list">
            <article class="pledge" data-pledge-id="12345678">
                <h2 class="pledge-name">Aegis Avenger Titan</h2>
                <span class="pledge-manufacturer">Aegis Dynamics</span>
                <span class="pledge-kind">Standalone Ship</span>
            </article>
            <article class="pledge" data-pledge-id="87654321">
                <h2 class="pledge-name">Greycat PTV</h2>
                <span class="pledge-manufacturer">Greycat Industrial</span>
                <span class="pledge-kind">Ground Vehicle</span>
            </article>
            <article class="pledge" data-pledge-id="55555555">
                <h2 class="pledge-name">Mystery Skin</h2>
            </article>
        </div>
        </body></html>
    "#;

    #[test]
    fn parse_pledges_extracts_ships_with_all_fields() {
        let parsed = parse_pledges_html(FULL_FIXTURE);
        assert_eq!(parsed.len(), 3);

        // Order is preserved from the page.
        assert_eq!(parsed[0].name, "Aegis Avenger Titan");
        assert_eq!(parsed[0].manufacturer.as_deref(), Some("Aegis Dynamics"));
        assert_eq!(parsed[0].kind.as_deref(), Some("Standalone Ship"));
        assert_eq!(parsed[0].pledge_id.as_deref(), Some("12345678"));

        assert_eq!(parsed[1].name, "Greycat PTV");
        assert_eq!(
            parsed[1].manufacturer.as_deref(),
            Some("Greycat Industrial")
        );
        assert_eq!(parsed[1].kind.as_deref(), Some("Ground Vehicle"));
        assert_eq!(parsed[1].pledge_id.as_deref(), Some("87654321"));

        // Sparse entry: only name + pledge_id.
        assert_eq!(parsed[2].name, "Mystery Skin");
        assert_eq!(parsed[2].manufacturer, None);
        assert_eq!(parsed[2].kind, None);
        assert_eq!(parsed[2].pledge_id.as_deref(), Some("55555555"));
    }

    #[test]
    fn parse_pledges_drops_entries_without_name() {
        // Mix of well-formed + malformed entries. The parser must skip
        // anything missing the only required field (name) without
        // collapsing the entire result.
        const FIXTURE: &str = r#"
            <html><body>
            <div class="pledge-list">
                <article class="pledge" data-pledge-id="111">
                    <span class="pledge-manufacturer">Orphan No Name</span>
                </article>
                <article class="pledge" data-pledge-id="222">
                    <h2 class="pledge-name">Valid Ship</h2>
                </article>
                <article class="pledge" data-pledge-id="333">
                    <h2 class="pledge-name">   </h2>
                    <span class="pledge-manufacturer">Whitespace Name</span>
                </article>
            </div>
            </body></html>
        "#;
        let parsed = parse_pledges_html(FIXTURE);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Valid Ship");
        assert_eq!(parsed[0].pledge_id.as_deref(), Some("222"));
    }

    #[test]
    fn parse_pledges_handles_empty_hangar() {
        // A user with no pledges lands on a page that renders the shell
        // but no `.pledge` containers. Parser must return an empty Vec,
        // not panic.
        const FIXTURE: &str = r#"
            <html><body>
            <div class="pledge-list">
                <div class="empty-state">
                    <p>You have no pledges yet.</p>
                </div>
            </div>
            </body></html>
        "#;
        let parsed = parse_pledges_html(FIXTURE);
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_pledges_returns_empty_for_garbage() {
        // Defensive: arbitrary HTML with no recognisable pledge markup
        // returns an empty Vec rather than panicking. Catches the case
        // where RSI returns a 200 with a maintenance page or a totally
        // restructured layout.
        const FIXTURE: &str = "<html><body><h1>Hello</h1><p>Nothing to see here.</p></body></html>";
        let parsed = parse_pledges_html(FIXTURE);
        assert!(parsed.is_empty());
    }

    #[test]
    fn sanitise_drops_ships_with_oversize_fields() {
        let oversized = "x".repeat(MAX_FIELD_CHARS + 1);
        let ships = vec![
            HangarShip {
                name: "Good Ship".into(),
                manufacturer: Some("Aegis".into()),
                pledge_id: Some("1".into()),
                kind: Some("Ship".into()),
            },
            HangarShip {
                name: oversized.clone(),
                manufacturer: None,
                pledge_id: None,
                kind: None,
            },
            HangarShip {
                name: "Oversize Manufacturer".into(),
                manufacturer: Some(oversized.clone()),
                pledge_id: None,
                kind: None,
            },
            HangarShip {
                name: "".into(),
                manufacturer: None,
                pledge_id: None,
                kind: None,
            },
        ];
        let sanitised = sanitise_ships(ships);
        assert_eq!(sanitised.len(), 1);
        assert_eq!(sanitised[0].name, "Good Ship");
    }
}
