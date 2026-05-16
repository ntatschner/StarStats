//! Star Citizen vehicle reference data fetched from the Wiki API.
//!
//! Game events store internal class names like
//! `AEGS_Avenger_Stalker_Living` — the dashboard wants to render
//! "Aegis Avenger Stalker" instead. The [`ReferenceClient`] trait
//! fronts the upstream lookup so the daily refresh job can be
//! tested without hitting the network. Production
//! [`WikiReferenceClient`] paginates through
//! `https://api.star-citizen.wiki/api/v3/vehicles` and returns the
//! full vehicle catalogue as a single `Vec`.
//!
//! Failure modes deliberately collapse to
//! [`ReferenceFetchOutcome::UpstreamUnavailable`]: the caller logs
//! and falls back to whatever's already in the store. There is no
//! fine-grained error taxonomy because the only thing the caller
//! needs to know is "did we get fresh data, or are we still on the
//! stale cache." The trade-off mirrors `rsi_verify::HttpRsiClient`.
//!
//! Per-vehicle JSON parsing lives in the pure [`parse_vehicles_page`]
//! function so the test suite can exercise it without standing up a
//! mock HTTP server. The HTTP layer is a thin shell around
//! `reqwest` + this parser.

use async_trait::async_trait;
use std::time::Duration;

/// One vehicle pulled from the Wiki API. Field shape matches the
/// `vehicle_reference` table (see `migrations/0012_reference_data.sql`).
///
/// `class_name` is the internal Star Citizen class identifier and the
/// join key against event payloads. It's case-sensitive on the way
/// in; the store performs case-insensitive lookups via the
/// `lower(class_name)` index since game logs occasionally vary case.
///
/// All metadata fields except `display_name` are `Option`: the Wiki
/// API returns inconsistent shapes per vehicle and we'd rather store
/// `None` than synthesise a value the upstream didn't actually
/// publish. Empty / whitespace-only strings collapse to `None` at
/// parse time so the storage layer never sees `Some("")`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct VehicleReference {
    /// Internal game class name (e.g. "AEGS_Avenger_Stalker"). Used as
    /// the join key against event payloads. Case-sensitive on the way
    /// in, but the store lookups are case-insensitive (lower() index).
    pub class_name: String,
    /// Player-friendly name from the Wiki ("Aegis Avenger Stalker").
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hull_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}

/// Result of a fetch against the upstream Wiki. Two outcomes:
/// either we got the full catalogue (possibly empty), or the upstream
/// is unavailable / misbehaving and the caller should keep serving
/// whatever's already cached. There is deliberately no
/// "partial success" variant — a half-paginated walk is worse than
/// no refresh at all because it would corrupt the cache by deleting
/// vehicles that simply hadn't been fetched yet (if the caller
/// implements a "delete missing" policy in a future slice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceFetchOutcome {
    Vehicles(Vec<VehicleReference>),
    UpstreamUnavailable,
}

/// Top-level category an entry in the generic `reference_registry`
/// belongs to. Mirrors the `reference_registry_category_chk` CHECK
/// constraint in migration 0022 — adding a category requires a
/// follow-up migration to widen the allow-list.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceCategory {
    Vehicle,
    Weapon,
    Item,
    Location,
}

// `as_str` / `parse` are wired in by the store refactor (P2) and
// route layer (P4) — silence dead-code during the transition.
#[allow(dead_code)]
impl ReferenceCategory {
    /// Lowercase string form — the value stored in the `category`
    /// column and used in the public route segment.
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceCategory::Vehicle => "vehicle",
            ReferenceCategory::Weapon => "weapon",
            ReferenceCategory::Item => "item",
            ReferenceCategory::Location => "location",
        }
    }

    /// Parse from the route segment. Returns `None` on any value
    /// outside the CHECK-constraint allow-list so route handlers can
    /// 404 unknown categories rather than letting them reach the DB.
    pub fn parse(s: &str) -> Option<ReferenceCategory> {
        match s {
            "vehicle" => Some(ReferenceCategory::Vehicle),
            "weapon" => Some(ReferenceCategory::Weapon),
            "item" => Some(ReferenceCategory::Item),
            "location" => Some(ReferenceCategory::Location),
            _ => None,
        }
    }
}

/// A single entry in the generic reference registry. Per-category
/// extras live in `metadata` as a JSON object — schema-on-read — so
/// new categories can ship without DDL. `VehicleReference` (above)
/// remains the typed view callers use for vehicle-specific rendering;
/// once the store refactor lands it will be decoded from a
/// `ReferenceEntry` with `category == Vehicle`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ReferenceEntry {
    pub category: ReferenceCategory,
    pub class_name: String,
    pub display_name: String,
    /// JSON object holding per-category extras (manufacturer, role,
    /// size, slot, parent system…). `Default::default()` returns the
    /// empty object so unrenderable fields don't appear at all in
    /// JSON output. `serde_json::Value` does not implement `Eq`
    /// because of `f64`, so `ReferenceEntry` is `PartialEq` only.
    #[schema(value_type = Object)]
    #[serde(default, skip_serializing_if = "is_empty_object")]
    pub metadata: serde_json::Value,
}

fn is_empty_object(v: &serde_json::Value) -> bool {
    matches!(v, serde_json::Value::Object(m) if m.is_empty())
}

#[async_trait]
pub trait ReferenceClient: Send + Sync + 'static {
    /// Fetch the full vehicle reference set. Implementations are
    /// expected to paginate internally and return the full list as a
    /// single Vec. Failure modes collapse to UpstreamUnavailable; the
    /// caller logs and falls back to whatever's already in the store.
    ///
    /// Kept for backwards compatibility — new callers should prefer
    /// `fetch_category(ReferenceCategory::Vehicle)`. Unused by the
    /// in-tree cron after P3; the allow-dead silences the warning
    /// while the API stays available for external implementers.
    #[allow(dead_code)]
    async fn fetch_vehicles(&self) -> ReferenceFetchOutcome;

    /// Fetch the full catalogue for a single category. Returns
    /// generic `ReferenceEntry` items so callers can dispatch to one
    /// `upsert_entries` call regardless of category. Default impl
    /// reports the upstream as unavailable; the production
    /// `WikiReferenceClient` overrides it.
    async fn fetch_category(&self, _category: ReferenceCategory) -> ReferenceFetchOutcomeCategory {
        ReferenceFetchOutcomeCategory::UpstreamUnavailable
    }
}

/// Result of a generic category fetch. Mirrors `ReferenceFetchOutcome`
/// but holds a `Vec<ReferenceEntry>` so the caller can write all four
/// categories through a single store method. `serde_json::Value`
/// doesn't implement `Eq`, so this enum is `PartialEq` only.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceFetchOutcomeCategory {
    Entries(Vec<ReferenceEntry>),
    UpstreamUnavailable,
}

// The wiki's documented API base is `/api/...`, not `/api/v3/...`. The
// v3-prefixed vehicles route works as a legacy alias but the other
// categories don't have one — P3's first attempt 404'd on weapons /
// items / locations because of this. The OpenAPI spec at
// `/api/openapi` lists every endpoint without the v3 prefix.
const WIKI_VEHICLES_BASE: &str = "https://api.star-citizen.wiki/api/vehicles";
const WIKI_WEAPONS_BASE: &str = "https://api.star-citizen.wiki/api/weapons";
const WIKI_ITEMS_BASE: &str = "https://api.star-citizen.wiki/api/items";
const WIKI_LOCATIONS_BASE: &str = "https://api.star-citizen.wiki/api/locations";

/// Page size we request from the wiki. The server caps at 200 even if
/// you ask for more, and the default is 30 — passing `?limit=200`
/// keeps the request count tractable on items (20k+ entries) without
/// the round-trip-per-30 fan-out the default produces.
const WIKI_PAGE_LIMIT: u32 = 200;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Hard cap on how many pages we'll walk. Sized for the biggest
/// known catalogue (items, ~20k entries → ~101 pages at limit=200)
/// with 2x headroom for upstream growth and the occasional "the API
/// is misbehaving / paginating endlessly" abort case.
const MAX_PAGE_REQUESTS: u32 = 250;
/// Per-page body cap. Observed bodies at limit=200 are ~200 KB; 4 MB
/// leaves ample headroom without letting a misbehaving response
/// balloon a single allocation.
const MAX_PAGE_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Body cap across all pages combined. Items at limit=200 is
/// ~20 MB total; 200 MB leaves 10x headroom for upstream growth.
/// Enforced per-byte during streaming, not after `text()`
/// materialises the whole body.
const MAX_TOTAL_BODY_BYTES: usize = 200 * 1024 * 1024;
const USER_AGENT: &str = concat!(
    "StarStats/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/RSIStarCitizenTools/StarStats)"
);

/// Production [`ReferenceClient`] backed by `reqwest`. Holds a shared
/// client so connection pooling + DNS caching survive across calls
/// (the daily refresh job invokes `fetch_vehicles` once, but tests
/// and ad-hoc admin tooling may spin a single instance up and reuse).
pub struct WikiReferenceClient {
    inner: reqwest::Client,
}

impl WikiReferenceClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        let inner = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl ReferenceClient for WikiReferenceClient {
    async fn fetch_vehicles(&self) -> ReferenceFetchOutcome {
        let mut all = Vec::new();
        let mut page: u32 = 1;
        let mut total_bytes: usize = 0;

        loop {
            if page > MAX_PAGE_REQUESTS {
                tracing::warn!(
                    page,
                    cap = MAX_PAGE_REQUESTS,
                    "wiki vehicles paginated past safety cap; aborting"
                );
                return ReferenceFetchOutcome::UpstreamUnavailable;
            }

            let url = format!("{WIKI_VEHICLES_BASE}?page={page}&limit={WIKI_PAGE_LIMIT}");
            let resp = match self.inner.get(&url).send().await {
                Ok(r) => r,
                Err(err) => {
                    tracing::warn!(error = %err, page, "wiki vehicles fetch failed");
                    return ReferenceFetchOutcome::UpstreamUnavailable;
                }
            };

            let status = resp.status();
            if !status.is_success() {
                tracing::warn!(status = status.as_u16(), page, "wiki vehicles non-2xx");
                return ReferenceFetchOutcome::UpstreamUnavailable;
            }

            // Stream the body so we bail BEFORE allocating gigabytes
            // if the upstream misbehaves. `resp.text()` has no ceiling.
            let body = match read_capped_body(resp, page, total_bytes).await {
                Some(b) => b,
                None => return ReferenceFetchOutcome::UpstreamUnavailable,
            };
            total_bytes = total_bytes.saturating_add(body.len());

            let json: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!(error = %err, page, "wiki vehicles json parse failed");
                    return ReferenceFetchOutcome::UpstreamUnavailable;
                }
            };

            all.extend(parse_vehicles_page(&json));

            // Pagination terminates when current_page reaches
            // last_page. Defensive: missing meta = single-page mode.
            let meta = json.get("meta");
            let current_page = meta
                .and_then(|m| m.get("current_page"))
                .and_then(|v| v.as_u64())
                .unwrap_or(page as u64);
            let last_page = meta
                .and_then(|m| m.get("last_page"))
                .and_then(|v| v.as_u64())
                .unwrap_or(current_page);

            if current_page >= last_page {
                break;
            }
            page += 1;
        }

        ReferenceFetchOutcome::Vehicles(all)
    }

    async fn fetch_category(&self, category: ReferenceCategory) -> ReferenceFetchOutcomeCategory {
        let base = match category {
            ReferenceCategory::Vehicle => WIKI_VEHICLES_BASE,
            ReferenceCategory::Weapon => WIKI_WEAPONS_BASE,
            ReferenceCategory::Item => WIKI_ITEMS_BASE,
            ReferenceCategory::Location => WIKI_LOCATIONS_BASE,
        };
        let mut all: Vec<ReferenceEntry> = Vec::new();
        let mut page: u32 = 1;
        let mut total_bytes: usize = 0;

        loop {
            if page > MAX_PAGE_REQUESTS {
                tracing::warn!(
                    category = category.as_str(),
                    page,
                    cap = MAX_PAGE_REQUESTS,
                    "wiki paginated past safety cap; aborting"
                );
                return ReferenceFetchOutcomeCategory::UpstreamUnavailable;
            }

            let url = format!("{base}?page={page}&limit={WIKI_PAGE_LIMIT}");
            let resp = match self.inner.get(&url).send().await {
                Ok(r) => r,
                Err(err) => {
                    tracing::warn!(error = %err, category = category.as_str(), page, "wiki fetch failed");
                    return ReferenceFetchOutcomeCategory::UpstreamUnavailable;
                }
            };

            let status = resp.status();
            if !status.is_success() {
                tracing::warn!(
                    status = status.as_u16(),
                    category = category.as_str(),
                    page,
                    "wiki non-2xx"
                );
                return ReferenceFetchOutcomeCategory::UpstreamUnavailable;
            }

            let body = match read_capped_body(resp, page, total_bytes).await {
                Some(b) => b,
                None => return ReferenceFetchOutcomeCategory::UpstreamUnavailable,
            };
            total_bytes = total_bytes.saturating_add(body.len());

            let json: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!(error = %err, category = category.as_str(), page, "wiki json parse failed");
                    return ReferenceFetchOutcomeCategory::UpstreamUnavailable;
                }
            };

            all.extend(parse_category_page(&json, category));

            let meta = json.get("meta");
            let current_page = meta
                .and_then(|m| m.get("current_page"))
                .and_then(|v| v.as_u64())
                .unwrap_or(page as u64);
            let last_page = meta
                .and_then(|m| m.get("last_page"))
                .and_then(|v| v.as_u64())
                .unwrap_or(current_page);

            if current_page >= last_page {
                break;
            }
            page += 1;
        }

        ReferenceFetchOutcomeCategory::Entries(all)
    }
}

/// Stream a response body into a `Vec<u8>`, bailing out the moment it
/// crosses the per-page or cumulative cap. `reqwest::Response::text`
/// has no ceiling, so a misbehaving upstream could balloon a
/// server-side allocation. The cumulative limit is checked against the
/// running total carried across pages.
async fn read_capped_body(
    mut resp: reqwest::Response,
    page: u32,
    bytes_so_far: usize,
) -> Option<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len().saturating_add(chunk.len()) > MAX_PAGE_BODY_BYTES {
                    tracing::warn!(
                        cap_bytes = MAX_PAGE_BODY_BYTES,
                        page,
                        "wiki vehicles per-page body exceeded cap; aborting"
                    );
                    return None;
                }
                if bytes_so_far
                    .saturating_add(buf.len())
                    .saturating_add(chunk.len())
                    > MAX_TOTAL_BODY_BYTES
                {
                    tracing::warn!(
                        cap_bytes = MAX_TOTAL_BODY_BYTES,
                        "wiki vehicles cumulative body exceeded cap; aborting"
                    );
                    return None;
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => return Some(buf),
            Err(err) => {
                tracing::warn!(error = %err, page, "wiki vehicles body read failed");
                return None;
            }
        }
    }
}

/// Pull every well-formed vehicle out of a single Wiki API page.
///
/// Defensive on every field: the upstream JSON shape varies
/// per vehicle (some entries lack a manufacturer record, some have
/// `role` instead of `focus`, etc.) so we treat missing/null/empty
/// strings as `None` after trimming. The only hard requirement is a
/// non-empty `class_name` — without the join key the entry can't
/// link back to events, so it's dropped.
pub fn parse_vehicles_page(json: &serde_json::Value) -> Vec<VehicleReference> {
    let Some(data) = json.get("data").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(data.len());
    for entry in data {
        // Drop the entry the moment we can't lift a usable join key.
        let Some(class_name) = string_field(entry, "class_name") else {
            continue;
        };

        // Display name falls back to the class name only as a last
        // resort — a player would rather see "AEGS_Avenger_Stalker"
        // than nothing at all if the upstream record is half-formed.
        let display_name = string_field(entry, "name").unwrap_or_else(|| class_name.clone());

        let manufacturer = entry.get("manufacturer").and_then(|m| {
            // Preferred shape: nested object with `name` / `code`.
            // Fall back to a flat string if the upstream simplified
            // the field on that vehicle.
            if m.is_object() {
                string_field(m, "name").or_else(|| string_field(m, "code"))
            } else if m.is_string() {
                m.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            } else {
                None
            }
        });

        // Wiki vehicles publish the same field as `focus` on most
        // records and `role` on a minority — check both before giving
        // up. `type` is a third sibling field but it's coarser
        // ("MultiCrew Combat") and we keep it out so the role column
        // doesn't get noisy.
        let role = string_field(entry, "role").or_else(|| string_field(entry, "focus"));
        let focus = string_field(entry, "focus");
        let hull_size = string_field(entry, "size");

        out.push(VehicleReference {
            class_name,
            display_name,
            manufacturer,
            role,
            hull_size,
            focus,
        });
    }
    out
}

/// Generic per-page parser for any category. Pulls each item's class
/// identifier (with fallbacks: class_name → code → slug → ref) and
/// display name, then collects every remaining top-level field into
/// the metadata JSONB blob. Internal Wiki bookkeeping fields (`id`,
/// `created_at`, `updated_at`, `version`) are stripped so they don't
/// pollute the catalogue with noise the dashboard can't use.
///
/// Defensive shape: an entry without a usable class identifier is
/// dropped silently — without the join key it can't link back to an
/// event payload, so storing it would be inert.
pub fn parse_category_page(
    json: &serde_json::Value,
    category: ReferenceCategory,
) -> Vec<ReferenceEntry> {
    let Some(data) = json.get("data").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(data.len());
    for entry in data {
        let Some(obj) = entry.as_object() else {
            continue;
        };

        let class_name = string_field(entry, "class_name")
            .or_else(|| string_field(entry, "code"))
            .or_else(|| string_field(entry, "slug"))
            .or_else(|| string_field(entry, "ref"));
        let Some(class_name) = class_name else {
            continue;
        };

        let display_name = string_field(entry, "name").unwrap_or_else(|| class_name.clone());

        let mut metadata = serde_json::Map::new();
        for (k, v) in obj.iter() {
            if matches!(
                k.as_str(),
                "class_name" | "name" | "id" | "created_at" | "updated_at" | "version"
            ) {
                continue;
            }
            metadata.insert(k.clone(), v.clone());
        }

        out.push(ReferenceEntry {
            category,
            class_name,
            display_name,
            metadata: serde_json::Value::Object(metadata),
        });
    }
    out
}

/// Pull a string field from a JSON object, treating
/// missing/null/non-string/empty/whitespace-only as `None` after
/// trimming. Centralising this keeps the parser shape consistent —
/// the storage layer should never see `Some("")`.
fn string_field(obj: &serde_json::Value, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_page_extracts_all_fields() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    {
                        "id": 1,
                        "name": "Aegis Avenger Stalker",
                        "class_name": "AEGS_Avenger_Stalker",
                        "manufacturer": { "name": "Aegis Dynamics", "code": "AEGS" },
                        "size": "Small",
                        "focus": "Bounty Hunting",
                        "type": "MultiCrew Combat"
                    }
                ],
                "meta": { "current_page": 1, "last_page": 1, "total": 1 }
            }"#,
        )
        .unwrap();

        let parsed = parse_vehicles_page(&json);
        assert_eq!(parsed.len(), 1);
        let v = &parsed[0];
        assert_eq!(v.class_name, "AEGS_Avenger_Stalker");
        assert_eq!(v.display_name, "Aegis Avenger Stalker");
        assert_eq!(v.manufacturer.as_deref(), Some("Aegis Dynamics"));
        // `role` falls back to `focus` when no explicit `role` field
        // exists — mirrors the most common Wiki shape.
        assert_eq!(v.role.as_deref(), Some("Bounty Hunting"));
        assert_eq!(v.focus.as_deref(), Some("Bounty Hunting"));
        assert_eq!(v.hull_size.as_deref(), Some("Small"));
    }

    #[test]
    fn parse_multi_page_walks_each_page_independently() {
        // The parser is per-page — the page-walking loop lives in
        // `WikiReferenceClient::fetch_vehicles`. Synthesise two
        // pages here and concatenate them by hand to prove that two
        // independent calls compose into the expected flat Vec.
        let page1: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    { "name": "Aegis Avenger Stalker", "class_name": "AEGS_Avenger_Stalker" }
                ],
                "meta": { "current_page": 1, "last_page": 2, "total": 2 }
            }"#,
        )
        .unwrap();
        let page2: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    { "name": "Anvil Hornet", "class_name": "ANVL_Hornet_F7C" }
                ],
                "meta": { "current_page": 2, "last_page": 2, "total": 2 }
            }"#,
        )
        .unwrap();

        let mut combined = parse_vehicles_page(&page1);
        combined.extend(parse_vehicles_page(&page2));
        assert_eq!(combined.len(), 2);
        assert_eq!(combined[0].class_name, "AEGS_Avenger_Stalker");
        assert_eq!(combined[1].class_name, "ANVL_Hornet_F7C");
    }

    #[test]
    fn parse_drops_entries_missing_class_name() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    { "name": "No Class Name Here" },
                    { "name": "Empty Class", "class_name": "" },
                    { "name": "Whitespace Class", "class_name": "   " },
                    { "name": "Null Class", "class_name": null },
                    { "name": "Good One", "class_name": "AEGS_Gladius" }
                ]
            }"#,
        )
        .unwrap();

        let parsed = parse_vehicles_page(&json);
        // Only the last entry survives — every other shape lacks a
        // usable join key and is silently dropped.
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].class_name, "AEGS_Gladius");
    }

    #[test]
    fn parse_handles_missing_optional_fields() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    { "class_name": "AEGS_Bare", "name": "Bare Aegis" }
                ]
            }"#,
        )
        .unwrap();

        let parsed = parse_vehicles_page(&json);
        assert_eq!(parsed.len(), 1);
        let v = &parsed[0];
        assert_eq!(v.class_name, "AEGS_Bare");
        assert_eq!(v.display_name, "Bare Aegis");
        assert_eq!(v.manufacturer, None);
        assert_eq!(v.role, None);
        assert_eq!(v.focus, None);
        assert_eq!(v.hull_size, None);
    }

    #[test]
    fn parse_falls_back_display_name_to_class_name() {
        // Half-formed upstream record: no `name` field at all. Better
        // to surface the class name than nothing at all.
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    { "class_name": "AEGS_Mystery" }
                ]
            }"#,
        )
        .unwrap();
        let parsed = parse_vehicles_page(&json);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].display_name, "AEGS_Mystery");
    }

    #[test]
    fn parse_handles_string_manufacturer() {
        // Some upstream records flatten manufacturer into a bare
        // string instead of `{ name, code }`. The parser must accept
        // either shape.
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    {
                        "class_name": "DRAK_Cutlass_Black",
                        "name": "Drake Cutlass Black",
                        "manufacturer": "Drake Interplanetary"
                    }
                ]
            }"#,
        )
        .unwrap();
        let parsed = parse_vehicles_page(&json);
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0].manufacturer.as_deref(),
            Some("Drake Interplanetary")
        );
    }

    #[test]
    fn parse_empty_array_returns_empty_vec() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{ "data": [], "meta": { "current_page": 1, "last_page": 1, "total": 0 } }"#,
        )
        .unwrap();
        assert!(parse_vehicles_page(&json).is_empty());
    }

    #[test]
    fn parse_missing_data_array_returns_empty_vec() {
        // Defensive: a malformed upstream response (no `data` field
        // at all) shouldn't panic — it should yield an empty page.
        let json: serde_json::Value = serde_json::from_str(r#"{ "meta": {} }"#).unwrap();
        assert!(parse_vehicles_page(&json).is_empty());
    }

    #[test]
    fn parse_explicit_role_field_wins_over_focus() {
        // When a vehicle has both `role` and `focus`, prefer `role`
        // (the more specific field). `focus` is preserved separately.
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    {
                        "class_name": "AEGS_Vanguard",
                        "name": "Aegis Vanguard",
                        "role": "Heavy Fighter",
                        "focus": "Combat"
                    }
                ]
            }"#,
        )
        .unwrap();
        let parsed = parse_vehicles_page(&json);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].role.as_deref(), Some("Heavy Fighter"));
        assert_eq!(parsed[0].focus.as_deref(), Some("Combat"));
    }

    // -- parse_category_page (generic) --------------------------------

    #[test]
    fn parse_category_page_weapon_lifts_metadata() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    {
                        "id": 42,
                        "class_name": "KLWE_LaserCannon_S2",
                        "name": "Klaus & Werner Sledge II",
                        "manufacturer": { "name": "Klaus & Werner" },
                        "size": "S2",
                        "damage_type": "Energy"
                    }
                ],
                "meta": { "current_page": 1, "last_page": 1 }
            }"#,
        )
        .unwrap();
        let parsed = parse_category_page(&json, ReferenceCategory::Weapon);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].category, ReferenceCategory::Weapon);
        assert_eq!(parsed[0].class_name, "KLWE_LaserCannon_S2");
        assert_eq!(parsed[0].display_name, "Klaus & Werner Sledge II");
        let meta = parsed[0].metadata.as_object().unwrap();
        assert_eq!(meta.get("size").and_then(|v| v.as_str()), Some("S2"));
        assert_eq!(
            meta.get("damage_type").and_then(|v| v.as_str()),
            Some("Energy")
        );
        // Bookkeeping field stripped.
        assert!(meta.get("id").is_none());
    }

    #[test]
    fn parse_category_page_falls_back_to_code() {
        // Locations frequently use `code` rather than `class_name`.
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    { "code": "OOC_Stanton_2_Crusader", "name": "Crusader" }
                ]
            }"#,
        )
        .unwrap();
        let parsed = parse_category_page(&json, ReferenceCategory::Location);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].class_name, "OOC_Stanton_2_Crusader");
        assert_eq!(parsed[0].display_name, "Crusader");
    }

    #[test]
    fn parse_category_page_drops_entries_without_class_identifier() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "data": [
                    { "name": "Mystery item with no ID at all" },
                    { "class_name": "FOO_Bar", "name": "Foo Bar" }
                ]
            }"#,
        )
        .unwrap();
        let parsed = parse_category_page(&json, ReferenceCategory::Item);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].class_name, "FOO_Bar");
    }

    #[test]
    fn parse_category_page_falls_back_to_class_name_for_display() {
        // No `name` field — display defaults to the class name so the
        // dashboard never renders an empty cell.
        let json: serde_json::Value =
            serde_json::from_str(r#"{ "data": [{ "class_name": "ANVL_Hornet_F7C" }] }"#).unwrap();
        let parsed = parse_category_page(&json, ReferenceCategory::Vehicle);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].display_name, "ANVL_Hornet_F7C");
    }
}
