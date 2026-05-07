//! RSI public-profile bio scrape used to prove handle ownership.
//!
//! The [`RsiClient`] trait fronts the operation so handlers don't
//! depend on `reqwest` directly — tests substitute a fake
//! implementation that returns canned outcomes. The production
//! [`HttpRsiClient`] hits
//! `https://robertsspaceindustries.com/citizens/{handle}` and looks
//! for the verification code as a literal substring of the page body.
//!
//! Why substring search and not HTML parsing:
//! the code is `STARSTATS-XXXXXXXX` — 16+ characters, drawn from a
//! fixed alphabet, very unlikely to appear in any other context on
//! the page. Skipping a parser keeps us robust to RSI's frontend
//! reorganising bio markup (which they have, twice, in the last 18
//! months). The trade-off is we can't tell the user *where* their
//! code lives on the page; the trade-off is worth it.
//!
//! All upstream failures map to one of three outcomes that the
//! handler can render directly: [`BioContains`], [`BioMissing`],
//! [`HandleNotFound`], or [`UpstreamUnavailable`]. The handler is
//! deliberately not given the raw HTTP error — keeps logs clean and
//! reduces the surface for upstream-shaped error leakage.

use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::{Html, Selector};
use std::time::Duration;

/// Result of a single bio fetch + check. Drives the response of
/// `POST /v1/auth/rsi/verify` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsiCheckOutcome {
    /// Profile page loaded and contained the verification code.
    BioContains,
    /// Profile page loaded but the code wasn't present.
    BioMissing,
    /// RSI returned 404 for that handle (no such citizen).
    HandleNotFound,
    /// RSI was unreachable, returned 5xx, or the body couldn't be
    /// read. Maps to a 503 from the verify handler so clients know
    /// to retry rather than re-issue a code.
    UpstreamUnavailable,
}

/// Result of pulling the full citizen profile snapshot. Mirrors
/// [`RsiCheckOutcome`]'s upstream-failure shape so handlers can render
/// the same three states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RsiProfileOutcome {
    /// Profile page loaded and parsed.
    Found(RsiProfile),
    /// RSI returned 404 for that handle.
    HandleNotFound,
    /// RSI was unreachable, returned 5xx, or the body couldn't be
    /// read. Handler maps to 503 so clients retry rather than treat
    /// it as a permanent gap in the snapshot history.
    UpstreamUnavailable,
}

/// Parsed citizen profile. All fields are `Option<…>` because RSI
/// pages vary — some users hide their location, some haven't filled
/// in a bio, badge sets are entirely freeform — and we'd rather store
/// `NULL` than guess. Snapshot consumers should treat missing fields
/// as "not advertised", not "removed".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RsiProfile {
    pub display_name: Option<String>,
    pub enlistment_date: Option<NaiveDate>,
    pub location: Option<String>,
    pub badges: Vec<Badge>,
    pub bio: Option<String>,
    pub primary_org_summary: Option<String>,
}

/// One row out of the badge gallery on the citizen profile. `name`
/// is the badge's alt text (which RSI uses as its display label).
/// `image_url` is the absolute or page-relative URL of the icon —
/// we don't normalise it here because the storage layer is opaque
/// to URL shape and the renderer can decide whether to prepend the
/// origin.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct Badge {
    pub name: String,
    pub image_url: Option<String>,
}

/// One organisation listing pulled from a citizen's
/// `/citizens/{handle}/organizations` page.
///
/// `sid` is RSI's "Short ID" — a stable uppercase identifier for the
/// org (e.g. "TESTSQDN") that survives the org renaming. We use it as
/// the dedupe key when snapshotting; `name` may shift but `sid` won't
/// without explicit org-owner action. Both fields are required — if
/// the parser can't lift either we drop the entry.
///
/// `rank` is the member's rank within that org (e.g. "Senior Officer",
/// "Recruit"). Optional because the rank cell is occasionally missing
/// or hidden from the public view.
///
/// `is_main` distinguishes the user's primary org from affiliations.
/// Exactly one row per snapshot will have `is_main = true` (RSI
/// enforces single main org); affiliations all carry `false`. If the
/// upstream is malformed and we get zero or multiple "main" entries,
/// the parser preserves whatever it found — sanity-checking is the
/// store/handler's job.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct RsiOrg {
    /// Spectrum identifier (the immutable short slug RSI uses in URLs,
    /// e.g. `"AEGS"`). Stable across name changes — used as the join
    /// key when comparing snapshots.
    pub sid: String,
    /// Display name as shown on the RSI org page at the time of capture.
    pub name: String,
    /// User's rank within this org. `None` if the org publicly hides
    /// member ranks or the user has redacted theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    /// `true` if this is the user's "main" org (the one RSI features
    /// on the citizen card). At most one entry per snapshot has this
    /// set; the route layer enforces the invariant.
    pub is_main: bool,
}

/// Outcome of an `/organizations` fetch. Same shape as
/// [`RsiProfileOutcome`] for consistency at the route layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RsiOrgsOutcome {
    Found(Vec<RsiOrg>),
    HandleNotFound,
    UpstreamUnavailable,
}

#[async_trait]
pub trait RsiClient: Send + Sync + 'static {
    /// Fetch `https://robertsspaceindustries.com/citizens/{handle}`
    /// and report whether `code` appears in the body.
    async fn check_bio(&self, handle: &str, code: &str) -> RsiCheckOutcome;

    /// Fetch the same page and parse out the public citizen profile.
    /// The snapshot is taken once a user is verified; re-runs over
    /// time fill the snapshot history table.
    async fn fetch_profile(&self, handle: &str) -> RsiProfileOutcome;

    /// Fetch `https://robertsspaceindustries.com/citizens/{handle}/organizations`
    /// and parse out the user's main org + affiliations. Public scrape,
    /// no auth — same posture as `fetch_profile`. Empty `Found(vec![])`
    /// is a valid outcome (user has no public orgs, or hides them all).
    async fn fetch_orgs(&self, handle: &str) -> RsiOrgsOutcome;
}

const PROFILE_BASE: &str = "https://robertsspaceindustries.com/citizens";
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Hard cap on the citizen-page body. Live pages are ~150-200 KB; a
/// runaway response (misbehaving upstream, MITM injecting megabytes of
/// junk) shouldn't be allowed to balloon a server-side allocation.
const MAX_PROFILE_BODY_BYTES: usize = 2 * 1024 * 1024;
const USER_AGENT: &str = concat!(
    "StarStats/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/RSIStarCitizenTools/StarStats)"
);

/// Production [`RsiClient`] backed by `reqwest`. Holds a shared
/// client so connection pooling + DNS caching survive across calls.
pub struct HttpRsiClient {
    inner: reqwest::Client,
}

impl HttpRsiClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        let inner = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .user_agent(USER_AGENT)
            // Don't follow more than 5 redirects — RSI sometimes 30x's
            // expired profile URLs to a 404 page; the cap stops a
            // misconfigured upstream loop from holding our connection.
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl RsiClient for HttpRsiClient {
    async fn check_bio(&self, handle: &str, code: &str) -> RsiCheckOutcome {
        // Path-segment encoding so a handle like `Some User` survives
        // the request build without being mangled. `urlencoding` isn't
        // a workspace dep — `url::form_urlencoded` is in `reqwest`'s
        // tree but not exposed; hand-rolling for the narrow case is
        // shorter and more predictable.
        let url = format!("{}/{}", PROFILE_BASE, encode_path_segment(handle));

        let resp = match self.inner.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(error = %err, handle, "rsi fetch failed");
                return RsiCheckOutcome::UpstreamUnavailable;
            }
        };

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return RsiCheckOutcome::HandleNotFound;
        }
        if !status.is_success() {
            tracing::warn!(status = status.as_u16(), handle, "rsi non-2xx");
            return RsiCheckOutcome::UpstreamUnavailable;
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!(error = %err, handle, "rsi body read failed");
                return RsiCheckOutcome::UpstreamUnavailable;
            }
        };

        if body.contains(code) {
            RsiCheckOutcome::BioContains
        } else {
            RsiCheckOutcome::BioMissing
        }
    }

    async fn fetch_profile(&self, handle: &str) -> RsiProfileOutcome {
        let url = format!("{}/{}", PROFILE_BASE, encode_path_segment(handle));

        let resp = match self.inner.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(error = %err, handle, "rsi profile fetch failed");
                return RsiProfileOutcome::UpstreamUnavailable;
            }
        };

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return RsiProfileOutcome::HandleNotFound;
        }
        if !status.is_success() {
            tracing::warn!(status = status.as_u16(), handle, "rsi profile non-2xx");
            return RsiProfileOutcome::UpstreamUnavailable;
        }

        let body = match read_capped_text(resp, handle).await {
            Some(b) => b,
            None => return RsiProfileOutcome::UpstreamUnavailable,
        };

        RsiProfileOutcome::Found(parse_profile_html(&body))
    }

    async fn fetch_orgs(&self, handle: &str) -> RsiOrgsOutcome {
        // Same path-segment encoding posture as `fetch_profile` — the
        // handle slot can in theory contain anything the user typed, so
        // we percent-encode rather than rely on RSI's signup rules.
        let url = format!(
            "{}/{}/organizations",
            PROFILE_BASE,
            encode_path_segment(handle)
        );

        let resp = match self.inner.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(error = %err, handle, "rsi orgs fetch failed");
                return RsiOrgsOutcome::UpstreamUnavailable;
            }
        };

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return RsiOrgsOutcome::HandleNotFound;
        }
        if !status.is_success() {
            tracing::warn!(status = status.as_u16(), handle, "rsi orgs non-2xx");
            return RsiOrgsOutcome::UpstreamUnavailable;
        }

        let body = match read_capped_text(resp, handle).await {
            Some(b) => b,
            None => return RsiOrgsOutcome::UpstreamUnavailable,
        };

        // Empty Vec is a legitimate outcome: the user has no public
        // orgs (allowed in RSI). Distinguishing "no orgs" vs "markup
        // changed and parser dropped everything" is the route layer's
        // problem; from here we can't tell them apart.
        RsiOrgsOutcome::Found(parse_orgs_html(&body))
    }
}

/// Read the response body into a `String`, bailing out if it crosses
/// `MAX_PROFILE_BODY_BYTES`. `reqwest::Response::text` has no ceiling,
/// so a misbehaving upstream could balloon a server-side allocation.
async fn read_capped_text(mut resp: reqwest::Response, handle: &str) -> Option<String> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len().saturating_add(chunk.len()) > MAX_PROFILE_BODY_BYTES {
                    tracing::warn!(
                        handle,
                        cap_bytes = MAX_PROFILE_BODY_BYTES,
                        "rsi profile body exceeded cap; aborting"
                    );
                    return None;
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(error = %err, handle, "rsi profile body read failed");
                return None;
            }
        }
    }
    match String::from_utf8(buf) {
        Ok(s) => Some(s),
        Err(err) => {
            tracing::warn!(error = %err, handle, "rsi profile body not utf-8");
            None
        }
    }
}

// -- HTML parser helpers --------------------------------------------
//
// RSI's citizen page is server-rendered HTML; the markup has shifted
// twice in the last 18 months (cf. the substring rationale at the top
// of this file), so the parser is deliberately defensive: every
// selector falls back to `None` rather than propagating an error,
// and the caller stores whatever fields we managed to lift.
//
// Selector heuristics, all rooted under `.profile`:
//   * Display name + Handle live in the right column under
//     `.profile .info .entry .value`. There can be several `.entry`
//     blocks (Display Name, Handle name, Location, ...) so we walk
//     them and key off the `.label` text rather than depend on
//     positional :nth-child selectors which RSI has reshuffled.
//   * Enlistment date + Location + other "fact" rows live in the
//     left column under `.profile .left-col .entry`, again labelled.
//   * Bio is a single labelled entry whose value can be multi-line.
//   * Badges are a `<div class="badges">`-style strip of `<img>`
//     elements; alt text is the badge name. RSI sometimes hides
//     dud/empty trophies — we strip blanks so they never reach the
//     snapshot.
//   * Primary org lives under `.profile .right-col .main-org`; we
//     summarise it as "Name (rank)" / "Name" rather than carve out a
//     full struct, because the snapshot only needs a label.
//
// We chose left-col entry-by-label lookup over `.profile .info .entry .value`
// for "Display Name" because the right-column `.info` block on a
// citizen page lists *handle*-related fields (Display Name, Handle
// name, Country/Region) and the ordering has been observed to differ
// between accounts that haven't set every field. Looking up by label
// keeps the parser robust against re-ordered or missing rows.

// -- Org listing parser ---------------------------------------------
//
// The orgs page renders one block per org. RSI's live markup
// distinguishes the user's main org from affiliations by separate
// containers (a `.main` block followed by an `.affiliation` list).
// Inside each block, the relevant fields are:
//   * the SID (uppercase short identifier) -- stable across renames
//   * the org's display name
//   * the user's rank within that org (optional)
//
// Selectors here are deliberately defensive (label-led where possible,
// class-led where the markup carries no labels). If a future RSI
// reshuffle breaks the heuristics the parser returns an empty Vec
// rather than panicking; the route layer maps "empty parse from a 200
// page" to "user has no public orgs", which is also a valid real-world
// state, so the failure mode is benign even if it temporarily masks a
// markup change. Unit tests lock the contract so the change gets
// caught at CI time when the fixture is updated to mirror the new
// markup.
//
// Fixture-driven contract (see tests below):
//   * Each org sits inside a `.org` container.
//   * The container carries a modifier class — `.org--main` for the
//     primary org, `.org--affiliation` (or `.affiliation`) for the
//     rest.
//   * SID lives in `.org-sid` (or any descendant of the container with
//     class `.sid`).
//   * Display name lives in `.org-name`.
//   * Rank lives in `.org-rank`; absence is fine.
//
// Real RSI HTML is more verbose, but the parser only depends on the
// distinguishing class names above, not on the broader DOM shape.

fn parse_orgs_html(body: &str) -> Vec<RsiOrg> {
    let doc = Html::parse_document(body);

    // Top-level container selector. `.org` is intentionally generic so
    // either `.org--main` or `.org--affiliation` (and their bare-class
    // siblings `.main`, `.affiliation` if RSI ever drops the BEM
    // prefix) match through descendant rules below.
    let Ok(org_sel) = Selector::parse(".org") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for el in doc.select(&org_sel) {
        let Some(parsed) = parse_org_block(&el) else {
            continue;
        };
        out.push(parsed);
    }
    out
}

fn parse_org_block(el: &scraper::ElementRef<'_>) -> Option<RsiOrg> {
    let sid = find_first_text(el, &[".org-sid", ".sid"])?;
    let sid = sid.trim().to_owned();
    if sid.is_empty() {
        return None;
    }

    let name = find_first_text(el, &[".org-name", ".name"])?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return None;
    }

    let rank = find_first_text(el, &[".org-rank", ".rank"])
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    // The "main" flag is signalled by a modifier class on the
    // container. Accept both BEM-style (`org--main`) and plain
    // (`main`) so a future RSI reshuffle that drops the BEM prefix
    // doesn't silently demote every primary org to an affiliation.
    let is_main = el
        .value()
        .classes()
        .any(|c| c == "org--main" || c == "main");

    Some(RsiOrg {
        sid,
        name,
        rank,
        is_main,
    })
}

/// Walk the candidate selectors in order, return the trimmed text of
/// the first matching descendant. Used so a single org block can carry
/// either the BEM-prefixed class (`.org-sid`) or the bare class
/// (`.sid`) without the parser caring which.
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

fn parse_profile_html(body: &str) -> RsiProfile {
    let doc = Html::parse_document(body);

    let display_name = find_labelled_value(&doc, "Handle name")
        .or_else(|| find_labelled_value(&doc, "Display Name"));
    let enlistment_date = find_labelled_value(&doc, "Enlisted")
        .as_deref()
        .and_then(parse_enlistment_date);
    let location =
        find_labelled_value(&doc, "Location").or_else(|| find_labelled_value(&doc, "Country"));
    let bio = find_bio(&doc);
    let badges = find_badges(&doc);
    let primary_org_summary = find_primary_org_summary(&doc);

    RsiProfile {
        display_name,
        enlistment_date,
        location,
        badges,
        bio,
        primary_org_summary,
    }
}

/// Walk every `.entry` under `.profile`, return the trimmed `.value`
/// text of the first one whose `.label` matches `label` (case- and
/// trailing-colon-insensitive). RSI sometimes formats labels as
/// "Enlisted" and sometimes as "Enlisted:" — normalising both makes
/// the lookup robust.
fn find_labelled_value(doc: &Html, label: &str) -> Option<String> {
    // `Selector::parse` only fails on malformed CSS. The literals
    // here are all static, so an `unwrap` is safe — but we still
    // guard with `ok()?` so a future selector typo degrades to "no
    // match" rather than a panic in a hot path.
    let entry_sel = Selector::parse(".profile .entry").ok()?;
    let label_sel = Selector::parse(".label").ok()?;
    let value_sel = Selector::parse(".value").ok()?;

    let target = label.trim().trim_end_matches(':').to_ascii_lowercase();

    for entry in doc.select(&entry_sel) {
        let Some(label_el) = entry.select(&label_sel).next() else {
            continue;
        };
        let label_text = collect_text(&label_el)
            .trim()
            .trim_end_matches(':')
            .to_ascii_lowercase();
        if label_text != target {
            continue;
        }
        if let Some(value_el) = entry.select(&value_sel).next() {
            let value = collect_text(&value_el).trim().to_owned();
            if value.is_empty() {
                return None;
            }
            return Some(value);
        }
    }
    None
}

/// Bio is the only multi-line field on the page; collapse the inner
/// runs of whitespace so the stored copy is one stable string.
fn find_bio(doc: &Html) -> Option<String> {
    let value = find_labelled_value(doc, "Bio").or_else(|| {
        // Fallback: some templates render bio as `.entry.bio .value`
        // without a sibling `.label` we can match by text.
        let sel = Selector::parse(".profile .entry.bio .value").ok()?;
        doc.select(&sel)
            .next()
            .map(|el| collect_text(&el).trim().to_owned())
            .filter(|s| !s.is_empty())
    })?;

    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn find_badges(doc: &Html) -> Vec<Badge> {
    let Ok(sel) = Selector::parse(".profile .badges img") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for img in doc.select(&sel) {
        let alt = img
            .value()
            .attr("alt")
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let src = img
            .value()
            .attr("src")
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .and_then(normalise_badge_src);
        if let Some(name) = alt {
            out.push(Badge {
                name,
                image_url: src,
            });
        }
    }
    out
}

/// RSI badge `<img src=...>` attributes can be path-relative
/// (`/media/...`), absolute on RSI's CDN, or—if upstream HTML is ever
/// tampered with—anything else (`data:image/svg+xml,<svg
/// onload=...>`, `javascript:...`). Front-ends drop the value into an
/// `<img src>` so anything not anchored to RSI's own origin is an XSS
/// vector. We resolve to a fully-qualified RSI URL or drop the value.
fn normalise_badge_src(raw: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(stripped) = trimmed.strip_prefix("//") {
        let absolute = format!("https://{stripped}");
        return is_rsi_url(&absolute).then_some(absolute);
    }
    if trimmed.starts_with('/') {
        return Some(format!("https://robertsspaceindustries.com{trimmed}"));
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return is_rsi_host(rest.split('/').next().unwrap_or("")).then(|| trimmed.to_owned());
    }
    None
}

fn is_rsi_url(absolute: &str) -> bool {
    absolute
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(is_rsi_host)
}

fn is_rsi_host(host: &str) -> bool {
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    host == "robertsspaceindustries.com" || host.ends_with(".robertsspaceindustries.com")
}

fn find_primary_org_summary(doc: &Html) -> Option<String> {
    // The main-org block carries the org name in `.entry .value`
    // under `.profile .right-col .main-org .info`. Some pages don't
    // render this section at all (no main org), in which case we
    // return None rather than an empty string.
    let sel = Selector::parse(".profile .right-col .main-org .info .entry .value").ok()?;
    let value = doc
        .select(&sel)
        .next()
        .map(|el| collect_text(&el).trim().to_owned())?;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
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

/// RSI prints enlistment dates as `Mar 14, 2014`. The trailing
/// nbsp/tab whitespace shows up too, so trim before parsing.
fn parse_enlistment_date(raw: &str) -> Option<NaiveDate> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(cleaned, "%b %d, %Y").ok()
}

/// URL-encode a single path segment. RSI handles are alnum + `_`/`-`
/// per their signup rules, but we don't want to bake that assumption
/// in here — strict percent-encoding for everything outside the
/// unreserved set per RFC 3986.
fn encode_path_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.as_bytes() {
        let c = *b;
        let unreserved = c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(c as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{:02X}", c);
        }
    }
    out
}

/// Generate a `STARSTATS-XXXXXXXX` verification code. 8 hex chars
/// drawn from `rand`'s thread-local CSPRNG — 32 bits of entropy is
/// fine because the code is bound to a single user row and expires
/// in 30 minutes; brute force isn't a meaningful threat model here.
pub fn generate_verify_code() -> String {
    use rand::{thread_rng, RngCore};
    let mut bytes = [0u8; 4];
    thread_rng().fill_bytes(&mut bytes);
    format!(
        "STARSTATS-{:02X}{:02X}{:02X}{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_path_segment_passes_unreserved() {
        assert_eq!(encode_path_segment("TheCodeSaiyan"), "TheCodeSaiyan");
        assert_eq!(encode_path_segment("a_b-c.d~e"), "a_b-c.d~e");
    }

    #[test]
    fn encode_path_segment_percent_encodes_others() {
        assert_eq!(encode_path_segment("Some User"), "Some%20User");
        assert_eq!(encode_path_segment("a/b"), "a%2Fb");
    }

    #[test]
    fn generate_verify_code_has_expected_shape() {
        let code = generate_verify_code();
        assert!(code.starts_with("STARSTATS-"));
        assert_eq!(code.len(), "STARSTATS-".len() + 8);
        let suffix = &code["STARSTATS-".len()..];
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_verify_code_is_random() {
        // 32 bits of entropy: collisions are theoretically possible
        // but vanishingly unlikely across 100 calls (<2e-7).
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            assert!(seen.insert(generate_verify_code()));
        }
    }

    /// Stripped-down version of the live citizen page. Keeps only the
    /// nodes the parser walks; deliberately exercises the labelled
    /// `.entry` lookup, multi-line bio collapse, badge alt/src
    /// extraction, and the main-org summary path.
    const FULL_FIXTURE: &str = r#"
        <html><body>
        <div class="profile">
            <div class="left-col">
                <div class="entry">
                    <span class="label">Handle name</span>
                    <span class="value">TheCodeSaiyan</span>
                </div>
                <div class="entry">
                    <span class="label">Enlisted</span>
                    <span class="value">Mar 14, 2014</span>
                </div>
                <div class="entry">
                    <span class="label">Location</span>
                    <span class="value">United Kingdom, England</span>
                </div>
                <div class="entry bio">
                    <span class="label">Bio</span>
                    <span class="value">Line one.
                        Line two with   extra spaces.</span>
                </div>
                <div class="badges">
                    <img src="/badges/founder.png" alt="Original Backer" />
                    <img src="/badges/dev.png" alt="Developer" />
                    <img src="" alt="" />
                </div>
            </div>
            <div class="right-col">
                <div class="main-org">
                    <div class="info">
                        <div class="entry">
                            <span class="label">Name</span>
                            <span class="value">Imperium</span>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
    "#;

    #[test]
    fn parse_profile_html_extracts_all_fields() {
        let parsed = parse_profile_html(FULL_FIXTURE);
        assert_eq!(parsed.display_name.as_deref(), Some("TheCodeSaiyan"));
        assert_eq!(
            parsed.enlistment_date,
            Some(chrono::NaiveDate::from_ymd_opt(2014, 3, 14).unwrap())
        );
        assert_eq!(parsed.location.as_deref(), Some("United Kingdom, England"));
        // Multi-line bio collapsed to a single space-separated string.
        assert_eq!(
            parsed.bio.as_deref(),
            Some("Line one. Line two with extra spaces.")
        );
        // Empty alt is dropped; two real badges remain.
        assert_eq!(parsed.badges.len(), 2);
        assert_eq!(parsed.badges[0].name, "Original Backer");
        // Page-relative `/badges/...` is rewritten to the absolute
        // RSI-anchored URL so the renderer can drop it into `<img src>`
        // without the browser resolving it against StarStats's origin.
        assert_eq!(
            parsed.badges[0].image_url.as_deref(),
            Some("https://robertsspaceindustries.com/badges/founder.png")
        );
        assert_eq!(parsed.badges[1].name, "Developer");
        assert_eq!(parsed.primary_org_summary.as_deref(), Some("Imperium"));
    }

    /// Sparser page: only the display-name row is set. Everything
    /// else must come back as `None` / empty so the snapshot stores
    /// `NULL` rather than synthesised values.
    const SPARSE_FIXTURE: &str = r#"
        <html><body>
        <div class="profile">
            <div class="left-col">
                <div class="entry">
                    <span class="label">Handle name</span>
                    <span class="value">QuietCitizen</span>
                </div>
            </div>
        </div>
        </body></html>
    "#;

    #[test]
    fn parse_profile_html_missing_fields_are_none() {
        let parsed = parse_profile_html(SPARSE_FIXTURE);
        assert_eq!(parsed.display_name.as_deref(), Some("QuietCitizen"));
        assert_eq!(parsed.enlistment_date, None);
        assert_eq!(parsed.location, None);
        assert_eq!(parsed.bio, None);
        assert!(parsed.badges.is_empty());
        assert_eq!(parsed.primary_org_summary, None);
    }

    /// Anything that isn't an https URL anchored under
    /// robertsspaceindustries.com (or an RSI-prefixed path) must be
    /// dropped — the renderer drops `image_url` into `<img src>`,
    /// where a `javascript:` / `data:image/svg+xml,<svg onload=…>`
    /// payload would execute in the user's browser. Tampered upstream
    /// HTML or a future RSI XSS shouldn't bridge into StarStats.
    #[test]
    fn parse_profile_html_drops_non_rsi_badge_urls() {
        const FIXTURE: &str = r#"
            <html><body>
                <div class="profile">
                    <div class="badges">
                        <img alt="Bad SVG" src="data:image/svg+xml,<svg onload=alert(1)>">
                        <img alt="Bad JS" src="javascript:alert(1)">
                        <img alt="Bad host" src="https://attacker.example/badge.png">
                        <img alt="Page-relative OK" src="/media/badge.png">
                        <img alt="Protocol-relative OK" src="//cdn.robertsspaceindustries.com/badge.png">
                        <img alt="Absolute OK" src="https://media.robertsspaceindustries.com/badge.png">
                    </div>
                </div>
            </body></html>
        "#;
        let parsed = parse_profile_html(FIXTURE);
        let by_name: std::collections::HashMap<_, _> = parsed
            .badges
            .iter()
            .map(|b| (b.name.as_str(), b.image_url.as_deref()))
            .collect();
        assert_eq!(by_name.get("Bad SVG"), Some(&None));
        assert_eq!(by_name.get("Bad JS"), Some(&None));
        assert_eq!(by_name.get("Bad host"), Some(&None));
        assert_eq!(
            by_name.get("Page-relative OK"),
            Some(&Some("https://robertsspaceindustries.com/media/badge.png"))
        );
        assert_eq!(
            by_name.get("Protocol-relative OK"),
            Some(&Some("https://cdn.robertsspaceindustries.com/badge.png"))
        );
        assert_eq!(
            by_name.get("Absolute OK"),
            Some(&Some("https://media.robertsspaceindustries.com/badge.png"))
        );
    }

    // -- Org listing parser fixtures ---------------------------------
    //
    // These fixtures are hand-crafted to lock the parser's contract
    // rather than mirror RSI's live markup verbatim. The contract:
    //   * Each org sits in a `.org` container.
    //   * `.org--main` modifier flags the primary org; everything else
    //     is treated as an affiliation.
    //   * `.org-sid` carries the stable Short ID (uppercase),
    //     `.org-name` the display name, `.org-rank` the (optional)
    //     rank.
    // If RSI's actual markup diverges, update the fixture and the
    // parser together — that's the whole point of locking the contract
    // here.

    const ORGS_FULL_FIXTURE: &str = r#"
        <html><body>
        <div class="orgs">
            <div class="org org--main">
                <span class="org-sid">IMP</span>
                <span class="org-name">Imperium</span>
                <span class="org-rank">Senior Officer</span>
            </div>
            <div class="org org--affiliation">
                <span class="org-sid">TESTSQDN</span>
                <span class="org-name">Test Squadron</span>
                <span class="org-rank">Recruit</span>
            </div>
            <div class="org org--affiliation">
                <span class="org-sid">FOOBAR</span>
                <span class="org-name">Foo Bar Industries</span>
            </div>
        </div>
        </body></html>
    "#;

    #[test]
    fn parse_orgs_html_extracts_main_and_affiliations() {
        let parsed = parse_orgs_html(ORGS_FULL_FIXTURE);
        assert_eq!(parsed.len(), 3);

        // Order is preserved from the page (main first, then
        // affiliations) — the route layer relies on this for rendering.
        assert_eq!(parsed[0].sid, "IMP");
        assert_eq!(parsed[0].name, "Imperium");
        assert_eq!(parsed[0].rank.as_deref(), Some("Senior Officer"));
        assert!(parsed[0].is_main);

        assert_eq!(parsed[1].sid, "TESTSQDN");
        assert_eq!(parsed[1].name, "Test Squadron");
        assert_eq!(parsed[1].rank.as_deref(), Some("Recruit"));
        assert!(!parsed[1].is_main);

        assert_eq!(parsed[2].sid, "FOOBAR");
        assert_eq!(parsed[2].name, "Foo Bar Industries");
        // Missing `.org-rank` -> None, not empty string.
        assert_eq!(parsed[2].rank, None);
        assert!(!parsed[2].is_main);
    }

    #[test]
    fn parse_orgs_html_drops_entries_without_sid() {
        // Mix of well-formed + malformed entries. The parser must skip
        // anything missing the join keys (sid or name) without
        // collapsing the entire result.
        const FIXTURE: &str = r#"
            <html><body>
            <div class="orgs">
                <div class="org org--main">
                    <span class="org-name">Orphan No SID</span>
                    <span class="org-rank">Captain</span>
                </div>
                <div class="org org--affiliation">
                    <span class="org-sid">VALID</span>
                    <span class="org-name">Valid Org</span>
                </div>
                <div class="org org--affiliation">
                    <span class="org-sid">NONAME</span>
                    <span class="org-rank">Ensign</span>
                </div>
                <div class="org org--affiliation">
                    <span class="org-sid">  </span>
                    <span class="org-name">Whitespace SID</span>
                </div>
            </div>
            </body></html>
        "#;
        let parsed = parse_orgs_html(FIXTURE);
        // Only the well-formed entry survives.
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].sid, "VALID");
        assert_eq!(parsed[0].name, "Valid Org");
    }

    #[test]
    fn parse_orgs_html_handles_no_orgs() {
        // A user with no public orgs lands on a page that renders the
        // shell but no `.org` containers (RSI shows an "Empty" state).
        // Parser must return an empty Vec, not panic.
        const FIXTURE: &str = r#"
            <html><body>
            <div class="orgs">
                <div class="empty-state">
                    <p>This citizen is not affiliated with any organization.</p>
                </div>
            </div>
            </body></html>
        "#;
        let parsed = parse_orgs_html(FIXTURE);
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_orgs_html_returns_empty_for_garbage() {
        // Defensive: arbitrary HTML with no recognisable org markup
        // returns an empty Vec rather than panicking. Catches the case
        // where RSI returns a 200 with a maintenance page or a totally
        // restructured layout.
        const FIXTURE: &str = "<html><body><h1>Hello</h1><p>Nothing to see here.</p></body></html>";
        let parsed = parse_orgs_html(FIXTURE);
        assert!(parsed.is_empty());
    }
}
