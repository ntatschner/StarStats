//! Read-side query API.
//!
//! Endpoints:
//!  - `GET /v1/me/events`  — paginated event stream for the caller.
//!  - `GET /v1/me/summary` — aggregated counts by `event_type`.
//!
//! Scoping: every query filters by the authenticated user's
//! `preferred_username`. Cross-user reads land in a separate
//! authorisation slice (SpiceDB) and route prefix.

use crate::api_error::ApiErrorBody;
use crate::auth::AuthenticatedUser;
use crate::locations::{self, ResolvedLocation, LOCATION_EVENT_TYPES};
use crate::repo::{
    EventFilters, EventQuery, EventTypeStats, InferredSession, IngestBatchRow, PayloadFieldBucket,
    PayloadFilter, SeqCursor,
};
use crate::spicedb::{ObjectRef, SpicedbClient};
use crate::validation::{build_timeline_buckets, is_valid_event_type, resolve_timeline_days};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    Extension,
};
#[cfg(test)]
use chrono::Duration;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

/// Hard caps for the events list endpoint.
const LIST_LIMIT_MAX: u32 = 500;
const LIST_LIMIT_MIN: u32 = 1;

/// Hard caps for the sessions list endpoint. Sessions are aggregates,
/// so even a heavy player won't have many — 500 trips covers years of
/// nightly play.
const SESSIONS_LIMIT_MAX: u32 = 500;
const SESSIONS_LIMIT_MIN: u32 = 1;
const SESSIONS_LIMIT_DEFAULT: u32 = 100;

/// Hard caps for the ingest-history list. Each row is one batch the
/// desktop client posted; an active player at the default 60s sync
/// interval generates ~1440/day, so 500 covers ~8 hours of play
/// without paginating. Heavier than that the user can page.
const INGEST_HISTORY_LIMIT_MAX: u32 = 500;
const INGEST_HISTORY_LIMIT_MIN: u32 = 1;
const INGEST_HISTORY_LIMIT_DEFAULT: u32 = 100;

/// Allowed `range` values on `GET /v1/me/metrics/event-types`. Mapped
/// to a `since=NOW() - days` filter; `all` skips the filter entirely.
const RANGE_OPTIONS: &[(&str, Option<i64>)] = &[
    ("7d", Some(7)),
    ("30d", Some(30)),
    ("90d", Some(90)),
    ("all", None),
];
const RANGE_DEFAULT: &str = "30d";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListParams {
    /// Legacy forward cursor: return events with `seq > after`.
    /// Kept for backwards compatibility; prefer `after_seq` /
    /// `before_seq` for new clients.
    #[serde(default)]
    pub after: Option<i64>,
    /// Cursor for "older" pagination: return events with
    /// `event_seq < before_seq`, ordered DESC by seq.
    #[serde(default)]
    pub before_seq: Option<i64>,
    /// Cursor for "newer" pagination: return events with
    /// `event_seq > after_seq`, ordered ASC by seq.
    #[serde(default)]
    pub after_seq: Option<i64>,
    /// Filter by exact event type. Validated as `[a-z0-9_]{1,64}`.
    #[serde(default)]
    pub event_type: Option<String>,
    /// Filter to events whose `event_timestamp` is at or after this
    /// instant.
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// Filter to events whose `event_timestamp` is at or before this
    /// instant.
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    100
}

fn err(status: StatusCode, code: &str) -> axum::response::Response {
    (
        status,
        Json(ApiErrorBody {
            error: code.to_string(),
            detail: None,
        }),
    )
        .into_response()
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TimelineParams {
    /// Number of trailing days to bucket. Defaults to 30, max 90.
    #[serde(default)]
    pub days: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TimelineResponse {
    pub days: u32,
    pub buckets: Vec<TimelineBucket>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TimelineBucket {
    /// ISO date `YYYY-MM-DD` in UTC.
    pub date: String,
    pub count: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventsListResponse {
    pub events: Vec<EventDto>,
    pub next_after: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventDto {
    pub seq: i64,
    pub event_type: String,
    pub event_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub log_source: String,
    pub source_offset: i64,
    /// Free-form JSON — variant of `starstats_core::events::GameEvent`,
    /// internally tagged on `type`.
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SummaryResponse {
    pub claimed_handle: String,
    pub total: u64,
    pub by_type: Vec<TypeCount>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TypeCount {
    pub event_type: String,
    pub count: u64,
}

#[utoipa::path(
    get,
    path = "/v1/me/events",
    tag = "query",
    params(ListParams),
    responses(
        (status = 200, description = "Paginated event stream for the caller", body = EventsListResponse),
        (status = 400, description = "Invalid filter or cursor combination", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn list_events<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    // Cap limit so a malicious client can't ask for everything in
    // one request and exhaust the connection pool.
    let limit = params.limit.clamp(LIST_LIMIT_MIN, LIST_LIMIT_MAX);

    // Validate event_type before we touch the DB.
    if let Some(t) = &params.event_type {
        if !is_valid_event_type(t) {
            return err(StatusCode::BAD_REQUEST, "invalid_event_type");
        }
    }

    // At most one cursor variant. If both new-style cursors are set,
    // 400. The legacy `after` is treated as `after_seq` only when
    // neither new-style cursor is set, so old clients still work.
    if params.before_seq.is_some() && params.after_seq.is_some() {
        return err(StatusCode::BAD_REQUEST, "conflicting_cursors");
    }

    let cursor = match (params.before_seq, params.after_seq, params.after) {
        (Some(_), Some(_), _) => unreachable!("conflict caught above"),
        (Some(b), None, _) => Some(SeqCursor::Before(b)),
        (None, Some(a), _) => Some(SeqCursor::After(a)),
        (None, None, Some(a)) if a > 0 => Some(SeqCursor::After(a)),
        _ => None,
    };

    let filters = EventFilters {
        cursor,
        event_type: params.event_type.clone(),
        since: params.since,
        until: params.until,
        limit: limit as i64,
    };

    match query.list_filtered(&user.preferred_username, filters).await {
        Ok(events) => {
            // `next_after` = the last seq the caller has now seen, so
            // they can pass it back as `after_seq` (or legacy `after`)
            // to fetch the next forward page.
            let next_after = events.iter().map(|e| e.seq).max();
            let dtos = events
                .into_iter()
                .map(|e| EventDto {
                    seq: e.seq,
                    event_type: e.event_type,
                    event_timestamp: e.event_timestamp,
                    log_source: e.log_source,
                    source_offset: e.source_offset,
                    payload: e.payload,
                })
                .collect();
            (
                StatusCode::OK,
                Json(EventsListResponse {
                    events: dtos,
                    next_after,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "list_events failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/me/timeline",
    tag = "query",
    params(TimelineParams),
    responses(
        (status = 200, description = "Per-day event counts for the trailing window", body = TimelineResponse),
        (status = 400, description = "Invalid `days` parameter", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn timeline<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<TimelineParams>,
) -> impl IntoResponse {
    let Ok(days) = resolve_timeline_days(params.days) else {
        return err(StatusCode::BAD_REQUEST, "invalid_days");
    };

    match query.timeline(&user.preferred_username, days).await {
        Ok(rows) => {
            let buckets = build_timeline_buckets(rows, days)
                .into_iter()
                .map(|(date, count)| TimelineBucket { date, count })
                .collect();
            (StatusCode::OK, Json(TimelineResponse { days, buckets })).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "timeline failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/me/summary",
    tag = "query",
    responses(
        (status = 200, description = "Aggregated counts by event type", body = SummaryResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "SpiceDB denied access to this stats record"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn summary<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
) -> impl IntoResponse {
    // Enforced SpiceDB check: every user implicitly has `view` on
    // their own `stats_record`, so the happy path is a no-op. A
    // denial here means the schema or relationship store has been
    // tampered with — return 403 rather than leak data. SpiceDB
    // outages fail open: this endpoint only exposes the caller's
    // own data, so paging on transient SpiceDB blips would be worse
    // than letting self-reads through.
    if let Some(client) = spicedb.as_ref() {
        let resource = ObjectRef::new("stats_record", &user.preferred_username);
        let subject = ObjectRef::new("user", &user.preferred_username);
        match client.check_permission(resource, "view", subject).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(
                    handle = %user.preferred_username,
                    "SpiceDB denied self-summary access"
                );
                return (StatusCode::FORBIDDEN, "forbidden").into_response();
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "SpiceDB check errored on self-summary; failing open"
                );
            }
        }
    }

    match query.summary_for_handle(&user.preferred_username).await {
        Ok((total, by_type)) => (
            StatusCode::OK,
            Json(SummaryResponse {
                claimed_handle: user.preferred_username,
                total,
                by_type: by_type
                    .into_iter()
                    .map(|(event_type, count)| TypeCount { event_type, count })
                    .collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "summary failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

// -- Metrics aggregates ---------------------------------------------
//
// Powers the web app's Metrics page (4 tabs: Overview, Event types,
// Sessions, Raw stream). Overview + Raw stream reuse the existing
// `/v1/me/{summary,events}` endpoints; the two routes below are the
// new aggregates the design needs.

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventTypeBreakdownParams {
    /// Time window: `7d`, `30d`, `90d`, or `all`. Defaults to `30d`.
    #[serde(default)]
    pub range: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventTypeBreakdownResponse {
    /// Echo of the resolved range — clients use it for the column
    /// header without a second round-trip.
    pub range: String,
    pub types: Vec<EventTypeStatsDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventTypeStatsDto {
    pub event_type: String,
    pub count: i64,
    pub last_seen: Option<DateTime<Utc>>,
}

impl From<EventTypeStats> for EventTypeStatsDto {
    fn from(s: EventTypeStats) -> Self {
        Self {
            event_type: s.event_type,
            count: s.count,
            last_seen: s.last_seen,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SessionsParams {
    #[serde(default = "default_sessions_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_sessions_limit() -> u32 {
    SESSIONS_LIMIT_DEFAULT
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionsResponse {
    pub sessions: Vec<SessionDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionDto {
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub event_count: i64,
}

impl From<InferredSession> for SessionDto {
    fn from(s: InferredSession) -> Self {
        Self {
            start_at: s.start_at,
            end_at: s.end_at,
            event_count: s.event_count,
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/me/metrics/event-types",
    tag = "metrics",
    params(EventTypeBreakdownParams),
    responses(
        (status = 200, description = "Per-event-type counts + last_seen for the chosen range", body = EventTypeBreakdownResponse),
        (status = 400, description = "Unknown range value", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn metrics_event_types<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<EventTypeBreakdownParams>,
) -> impl IntoResponse {
    let range_key = params.range.as_deref().unwrap_or(RANGE_DEFAULT);
    let Some(&(_, days)) = RANGE_OPTIONS.iter().find(|(k, _)| *k == range_key) else {
        return err(StatusCode::BAD_REQUEST, "invalid_range");
    };
    let since = days.map(|d| Utc::now() - chrono::Duration::days(d));

    match query
        .event_type_breakdown(&user.preferred_username, since)
        .await
    {
        Ok(rows) => (
            StatusCode::OK,
            Json(EventTypeBreakdownResponse {
                range: range_key.to_string(),
                types: rows.into_iter().map(EventTypeStatsDto::from).collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "metrics_event_types failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

// -- Ingest history --------------------------------------------------
//
// Powers the My logs page (Wave 11). Per the project's "no raw
// retention" decision, this is metadata-only — there are no
// per-line drill-down or batch-retry endpoints.

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct IngestHistoryParams {
    #[serde(default = "default_ingest_history_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_ingest_history_limit() -> u32 {
    INGEST_HISTORY_LIMIT_DEFAULT
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IngestHistoryResponse {
    pub batches: Vec<IngestBatchDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IngestBatchDto {
    pub seq: i64,
    pub occurred_at: DateTime<Utc>,
    pub batch_id: String,
    pub game_build: Option<String>,
    pub total: i64,
    pub accepted: i64,
    pub duplicate: i64,
    pub rejected: i64,
}

impl From<IngestBatchRow> for IngestBatchDto {
    fn from(r: IngestBatchRow) -> Self {
        Self {
            seq: r.seq,
            occurred_at: r.occurred_at,
            batch_id: r.batch_id,
            game_build: r.game_build,
            total: r.total,
            accepted: r.accepted,
            duplicate: r.duplicate,
            rejected: r.rejected,
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/me/ingest-history",
    tag = "metrics",
    params(IngestHistoryParams),
    responses(
        (status = 200, description = "Recent ingest batches the caller's clients posted", body = IngestHistoryResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn ingest_history<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<IngestHistoryParams>,
) -> impl IntoResponse {
    let limit = params
        .limit
        .clamp(INGEST_HISTORY_LIMIT_MIN, INGEST_HISTORY_LIMIT_MAX) as i64;
    let offset = params.offset as i64;

    match query
        .ingest_history_for_handle(&user.preferred_username, limit, offset)
        .await
    {
        Ok(rows) => (
            StatusCode::OK,
            Json(IngestHistoryResponse {
                batches: rows.into_iter().map(IngestBatchDto::from).collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "ingest_history failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/me/metrics/sessions",
    tag = "metrics",
    params(SessionsParams),
    responses(
        (status = 200, description = "Inferred play sessions, newest first", body = SessionsResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn metrics_sessions<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<SessionsParams>,
) -> impl IntoResponse {
    let limit = params.limit.clamp(SESSIONS_LIMIT_MIN, SESSIONS_LIMIT_MAX) as i64;
    let offset = params.offset as i64;

    match query
        .sessions_for_handle(&user.preferred_username, limit, offset)
        .await
    {
        Ok(sessions) => (
            StatusCode::OK,
            Json(SessionsResponse {
                sessions: sessions.into_iter().map(SessionDto::from).collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "metrics_sessions failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

// -- Location: "you are here" resolver -----------------------------
//
// Backs `GET /v1/me/location/current`. Returns 204 (No Content) when
// no location-bearing event has fired in the last
// [`LOCATION_STALENESS_MINUTES`] minutes — the UI uses 204 as the
// "no recent activity" signal so it can hide the pill rather than
// show a stale reading.
//
// The 90-minute window is a tradeoff:
//   - Short enough that yesterday's session doesn't surface as
//     "current" the next morning.
//   - Long enough to survive long quantum jumps or short alt-tabs
//     without the user perceiving the pill as flickery.

/// Maximum age of the source event before the endpoint reports 204.
const LOCATION_STALENESS_MINUTES: i64 = 90;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CurrentLocationResponse {
    pub location: ResolvedLocation,
}

#[utoipa::path(
    get,
    path = "/v1/me/location/current",
    tag = "metrics",
    operation_id = "location_current",
    responses(
        (status = 200, description = "Most recent location reading", body = CurrentLocationResponse),
        (status = 204, description = "No location-bearing event in the last 90 minutes"),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn location_current<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let latest = match query
        .latest_location(&user.preferred_username, LOCATION_EVENT_TYPES)
        .await
    {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "latest_location failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response();
        }
    };

    let Some(event) = latest.location_event else {
        // No location event ever recorded for this user.
        return StatusCode::NO_CONTENT.into_response();
    };

    // Staleness gate. Done at the handler — not at the repo — because
    // the threshold is a UX choice that may evolve, and the repo
    // shouldn't bake it into its semantics.
    let cutoff = Utc::now() - chrono::Duration::minutes(LOCATION_STALENESS_MINUTES);
    if event.event_timestamp < cutoff {
        return StatusCode::NO_CONTENT.into_response();
    }

    let Some(resolved) = locations::resolve(
        &event.event_type,
        &event.payload,
        event.event_timestamp,
        latest.shard_hint,
    ) else {
        // Payload didn't carry the expected fields. Treat as 204 so
        // the UI hides the pill rather than showing a half-built one.
        tracing::warn!(
            event_type = %event.event_type,
            "location payload could not be resolved",
        );
        return StatusCode::NO_CONTENT.into_response();
    };

    (
        StatusCode::OK,
        Json(CurrentLocationResponse { location: resolved }),
    )
        .into_response()
}

// -- Location: journey trace --------------------------------------
//
// Backs `GET /v1/me/location/trace`. Returns ordered location
// transitions in a window (default = last 24h). The handler resolves
// each raw event through `locations::resolve` and collapses adjacent
// rows that share the same (planet, city) into single "dwell" entries
// — so a player who pinged `LocationInventoryRequested` ten times in
// Lorville lands as one Lorville entry, not ten.

const TRACE_DEFAULT_HOURS: i64 = 24;
const TRACE_MAX_HOURS: i64 = 24 * 7;
const TRACE_LIMIT_DEFAULT: i64 = 200;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TraceParams {
    /// Hours to look back. Defaults to 24h; capped at one week.
    #[serde(default)]
    pub hours: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TraceEntry {
    pub planet: Option<String>,
    pub city: Option<String>,
    pub system: Option<String>,
    pub shard: Option<String>,
    pub source_event_type: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub event_count: u32,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TraceResponse {
    pub hours: i64,
    pub entries: Vec<TraceEntry>,
}

#[utoipa::path(
    get,
    path = "/v1/me/location/trace",
    tag = "metrics",
    operation_id = "location_trace",
    params(TraceParams),
    responses(
        (status = 200, description = "Ordered location trace, newest-first", body = TraceResponse),
        (status = 400, description = "Invalid hours window"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn location_trace<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<TraceParams>,
) -> impl IntoResponse {
    let hours = params.hours.unwrap_or(TRACE_DEFAULT_HOURS);
    if hours <= 0 || hours > TRACE_MAX_HOURS {
        return err(StatusCode::BAD_REQUEST, "invalid_hours");
    }
    let since = Utc::now() - chrono::Duration::hours(hours);

    // Walk the stream forward in time so adjacent same-location
    // events collapse cleanly into dwell entries. Then reverse for
    // newest-first response.
    let stream = match query
        .location_event_stream(&user.preferred_username, LOCATION_EVENT_TYPES, since)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "location_event_stream failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response();
        }
    };

    let entries = collapse_to_trace(stream);
    let mut entries = entries;
    entries.reverse();
    if entries.len() > TRACE_LIMIT_DEFAULT as usize {
        entries.truncate(TRACE_LIMIT_DEFAULT as usize);
    }

    (StatusCode::OK, Json(TraceResponse { hours, entries })).into_response()
}

/// Collapse a chronologically-ordered (oldest-first) stream of raw
/// location events into dwell entries. Two adjacent events that
/// resolve to the same `(planet, city)` pair become one entry whose
/// `started_at`/`ended_at` span both — and whose `event_count`
/// totals the underlying rows. A change in either field starts a
/// new entry.
fn collapse_to_trace(stream: Vec<crate::repo::LatestLocationEvent>) -> Vec<TraceEntry> {
    let mut out: Vec<TraceEntry> = Vec::new();
    for ev in stream {
        let Some(resolved) =
            locations::resolve(&ev.event_type, &ev.payload, ev.event_timestamp, None)
        else {
            continue;
        };
        let same_as_last = out.last().map_or(false, |prev| {
            prev.planet == resolved.planet
                && prev.city == resolved.city
                && prev.system == resolved.system
        });
        if same_as_last {
            let last = out.last_mut().unwrap();
            last.ended_at = ev.event_timestamp;
            last.event_count += 1;
        } else {
            out.push(TraceEntry {
                planet: resolved.planet,
                city: resolved.city,
                system: resolved.system,
                shard: resolved.shard,
                source_event_type: resolved.source_event_type,
                started_at: ev.event_timestamp,
                ended_at: ev.event_timestamp,
                event_count: 1,
            });
        }
    }
    out
}

// -- Location: aggregate breakdown ---------------------------------
//
// Backs `GET /v1/me/location/breakdown`. Sums dwell time per
// `(planet, city)` over a window. Dwell time is the gap between
// adjacent events at the same location, capped at the
// session-idle threshold so a logout doesn't bloat the dwell of the
// last-known place.

const BREAKDOWN_DEFAULT_HOURS: i64 = 24 * 7;
const BREAKDOWN_MAX_HOURS: i64 = 24 * 30;
/// Cap a single inter-event gap at this many minutes when summing
/// dwell. Anything longer is treated as "logged out" and contributes
/// only the cap — not the full gap.
const DWELL_CAP_MINUTES: i64 = 30;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BreakdownParams {
    #[serde(default)]
    pub hours: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct BreakdownEntry {
    pub planet: Option<String>,
    pub city: Option<String>,
    pub system: Option<String>,
    pub dwell_seconds: i64,
    pub visit_count: u32,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct BreakdownResponse {
    pub hours: i64,
    pub entries: Vec<BreakdownEntry>,
}

#[utoipa::path(
    get,
    path = "/v1/me/location/breakdown",
    tag = "metrics",
    operation_id = "location_breakdown",
    params(BreakdownParams),
    responses(
        (status = 200, description = "Aggregate dwell by location", body = BreakdownResponse),
        (status = 400, description = "Invalid hours window"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn location_breakdown<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<BreakdownParams>,
) -> impl IntoResponse {
    let hours = params.hours.unwrap_or(BREAKDOWN_DEFAULT_HOURS);
    if hours <= 0 || hours > BREAKDOWN_MAX_HOURS {
        return err(StatusCode::BAD_REQUEST, "invalid_hours");
    }
    let since = Utc::now() - chrono::Duration::hours(hours);

    let stream = match query
        .location_event_stream(&user.preferred_username, LOCATION_EVENT_TYPES, since)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "location_event_stream failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response();
        }
    };

    let entries = aggregate_dwell(stream);
    (StatusCode::OK, Json(BreakdownResponse { hours, entries })).into_response()
}

/// Walk an oldest-first stream and accumulate dwell time per
/// `(planet, city)` key. Each transition contributes the gap to the
/// PRIOR location, capped at [`DWELL_CAP_MINUTES`]. The terminal
/// location gets one cap-worth of dwell since we don't know when the
/// player left.
fn aggregate_dwell(stream: Vec<crate::repo::LatestLocationEvent>) -> Vec<BreakdownEntry> {
    use std::collections::BTreeMap;
    let cap = chrono::Duration::minutes(DWELL_CAP_MINUTES);
    let mut buckets: BTreeMap<(Option<String>, Option<String>, Option<String>), (i64, u32)> =
        BTreeMap::new();
    let mut prev: Option<(ResolvedLocation, DateTime<Utc>)> = None;
    for ev in stream {
        let Some(resolved) =
            locations::resolve(&ev.event_type, &ev.payload, ev.event_timestamp, None)
        else {
            continue;
        };
        if let Some((prev_loc, prev_ts)) = prev.take() {
            let gap = (ev.event_timestamp - prev_ts).min(cap);
            let secs = gap.num_seconds().max(0);
            let key = (
                prev_loc.planet.clone(),
                prev_loc.city.clone(),
                prev_loc.system.clone(),
            );
            let entry = buckets.entry(key).or_insert((0, 0));
            entry.0 += secs;
            entry.1 += 1;
        }
        prev = Some((resolved, ev.event_timestamp));
    }
    // Tail: contribute one cap-window of dwell to the terminal location.
    if let Some((last_loc, _)) = prev {
        let key = (last_loc.planet, last_loc.city, last_loc.system);
        let entry = buckets.entry(key).or_insert((0, 0));
        entry.0 += cap.num_seconds();
        entry.1 += 1;
    }

    let mut entries: Vec<BreakdownEntry> = buckets
        .into_iter()
        .map(|((planet, city, system), (dwell, visits))| BreakdownEntry {
            planet,
            city,
            system,
            dwell_seconds: dwell,
            visit_count: visits,
        })
        .collect();
    entries.sort_by(|a, b| b.dwell_seconds.cmp(&a.dwell_seconds));
    entries
}

// -- Activity stats: combat / travel / loadout / stability ---------
//
// One handler per stat family. Each is a thin wrapper around two
// repo calls: a `count_event_type` for the headline number plus a
// `payload_field_breakdown` for the secondary list. Bundling them
// here (rather than in their own modules) avoids fragmenting the
// query.rs surface — every read endpoint already lives in this file.

const STATS_DEFAULT_HOURS: i64 = 24 * 30;
const STATS_MAX_HOURS: i64 = 24 * 365;
const STATS_BUCKET_LIMIT: i64 = 10;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatsParams {
    #[serde(default)]
    pub hours: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StatsBucket {
    pub value: String,
    pub count: i64,
}

impl From<PayloadFieldBucket> for StatsBucket {
    fn from(b: PayloadFieldBucket) -> Self {
        Self {
            value: b.value,
            count: b.count,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CombatStatsResponse {
    pub hours: i64,
    /// Times the user appeared as the killer in `actor_death`.
    pub kills: u64,
    /// Times the user (or their character) appeared as the victim.
    pub deaths: u64,
    pub top_weapons: Vec<StatsBucket>,
    pub deaths_by_zone: Vec<StatsBucket>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TravelStatsResponse {
    pub hours: i64,
    pub quantum_jumps: u64,
    pub top_destinations: Vec<StatsBucket>,
    pub planets_visited: Vec<StatsBucket>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LoadoutStatsResponse {
    pub hours: i64,
    pub attachments: u64,
    pub top_items: Vec<StatsBucket>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StabilityStatsResponse {
    pub hours: i64,
    pub crashes: u64,
    pub by_channel: Vec<StatsBucket>,
}

fn parse_stats_window(params: &StatsParams) -> Result<(i64, Option<DateTime<Utc>>), Response> {
    let hours = params.hours.unwrap_or(STATS_DEFAULT_HOURS);
    if hours <= 0 || hours > STATS_MAX_HOURS {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_hours"));
    }
    let since = Some(Utc::now() - chrono::Duration::hours(hours));
    Ok((hours, since))
}

#[utoipa::path(
    get,
    path = "/v1/me/stats/combat",
    tag = "metrics",
    operation_id = "stats_combat",
    params(StatsParams),
    responses(
        (status = 200, description = "Combat stats", body = CombatStatsResponse),
        (status = 400, description = "Invalid hours window"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn stats_combat<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<StatsParams>,
) -> impl IntoResponse {
    let (hours, since) = match parse_stats_window(&params) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let handle = user.preferred_username.as_str();
    let killer_filter = PayloadFilter {
        field: "killer",
        equals: handle,
    };
    let victim_filter = PayloadFilter {
        field: "victim",
        equals: handle,
    };

    // Kills: actor_death rows where the caller is the killer.
    let kills = query
        .count_event_type(handle, "actor_death", Some(killer_filter), since)
        .await
        .unwrap_or(0);
    // Deaths: actor_death rows where the caller is the victim.
    let deaths = query
        .count_event_type(handle, "actor_death", Some(victim_filter), since)
        .await
        .unwrap_or(0);
    // Top weapons used by the caller — scoped to kills, otherwise
    // we'd be showing weapons that killed the caller (a different,
    // less-flattering stat that lives under deaths_by_zone next door).
    let top_weapons = query
        .payload_field_breakdown(
            handle,
            "actor_death",
            "weapon",
            Some(killer_filter),
            since,
            STATS_BUCKET_LIMIT,
        )
        .await
        .unwrap_or_default();
    // Deaths by zone: where the caller keeps getting jumped. Filtered
    // to victim==caller so a kill in a zone the caller cleared doesn't
    // inflate that zone's "danger" reading.
    let deaths_by_zone = query
        .payload_field_breakdown(
            handle,
            "actor_death",
            "zone",
            Some(victim_filter),
            since,
            STATS_BUCKET_LIMIT,
        )
        .await
        .unwrap_or_default();
    (
        StatusCode::OK,
        Json(CombatStatsResponse {
            hours,
            kills,
            deaths,
            top_weapons: top_weapons.into_iter().map(StatsBucket::from).collect(),
            deaths_by_zone: deaths_by_zone.into_iter().map(StatsBucket::from).collect(),
        }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/v1/me/stats/travel",
    tag = "metrics",
    operation_id = "stats_travel",
    params(StatsParams),
    responses(
        (status = 200, description = "Travel stats", body = TravelStatsResponse),
        (status = 400, description = "Invalid hours window"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn stats_travel<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<StatsParams>,
) -> impl IntoResponse {
    let (hours, since) = match parse_stats_window(&params) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let quantum_jumps = query
        .count_event_type(
            &user.preferred_username,
            "quantum_target_selected",
            None,
            since,
        )
        .await
        .unwrap_or(0);
    let top_destinations = query
        .payload_field_breakdown(
            &user.preferred_username,
            "quantum_target_selected",
            "destination",
            None,
            since,
            STATS_BUCKET_LIMIT,
        )
        .await
        .unwrap_or_default();
    let planets_visited = query
        .payload_field_breakdown(
            &user.preferred_username,
            "planet_terrain_load",
            "planet",
            None,
            since,
            STATS_BUCKET_LIMIT,
        )
        .await
        .unwrap_or_default();
    (
        StatusCode::OK,
        Json(TravelStatsResponse {
            hours,
            quantum_jumps,
            top_destinations: top_destinations
                .into_iter()
                .map(StatsBucket::from)
                .collect(),
            planets_visited: planets_visited.into_iter().map(StatsBucket::from).collect(),
        }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/v1/me/stats/loadout",
    tag = "metrics",
    operation_id = "stats_loadout",
    params(StatsParams),
    responses(
        (status = 200, description = "Loadout stats", body = LoadoutStatsResponse),
        (status = 400, description = "Invalid hours window"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn stats_loadout<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<StatsParams>,
) -> impl IntoResponse {
    let (hours, since) = match parse_stats_window(&params) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let attachments = query
        .count_event_type(&user.preferred_username, "attachment_received", None, since)
        .await
        .unwrap_or(0);
    let top_items = query
        .payload_field_breakdown(
            &user.preferred_username,
            "attachment_received",
            "item_class",
            None,
            since,
            STATS_BUCKET_LIMIT,
        )
        .await
        .unwrap_or_default();
    (
        StatusCode::OK,
        Json(LoadoutStatsResponse {
            hours,
            attachments,
            top_items: top_items.into_iter().map(StatsBucket::from).collect(),
        }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/v1/me/stats/stability",
    tag = "metrics",
    operation_id = "stats_stability",
    params(StatsParams),
    responses(
        (status = 200, description = "Stability stats", body = StabilityStatsResponse),
        (status = 400, description = "Invalid hours window"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn stats_stability<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<StatsParams>,
) -> impl IntoResponse {
    let (hours, since) = match parse_stats_window(&params) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let crashes = query
        .count_event_type(&user.preferred_username, "game_crash", None, since)
        .await
        .unwrap_or(0);
    let by_channel = query
        .payload_field_breakdown(
            &user.preferred_username,
            "game_crash",
            "channel",
            None,
            since,
            STATS_BUCKET_LIMIT,
        )
        .await
        .unwrap_or_default();
    (
        StatusCode::OK,
        Json(StabilityStatsResponse {
            hours,
            crashes,
            by_channel: by_channel.into_iter().map(StatsBucket::from).collect(),
        }),
    )
        .into_response()
}

/// Query params for `GET /v1/me/commerce/recent`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CommerceRecentParams {
    /// How many transactions to return. Capped at 500.
    #[serde(default = "default_commerce_limit")]
    pub limit: u32,
    /// Window for the "if no response in N seconds, mark timed out"
    /// classification. Mirrors the tray client's default of 30s.
    #[serde(default = "default_commerce_window_secs")]
    pub window_secs: i64,
}

fn default_commerce_limit() -> u32 {
    100
}
fn default_commerce_window_secs() -> i64 {
    30
}

/// Recent shop / commodity transactions for the caller, paired
/// `Send*Request` ↔ `*FlowResponse` via
/// [`starstats_core::pair_transactions`].
///
/// Strategy: pull the last ~1000 events (regardless of type) for the
/// user, deserialise each `payload`, filter to commerce variants,
/// then run the pure pairer. Commerce events are rare per-user so a
/// 1000-row cap covers a wide window without needing per-type
/// queries.
#[utoipa::path(
    get,
    path = "/v1/me/commerce/recent",
    tag = "query",
    params(CommerceRecentParams),
    responses(
        (status = 200, description = "Paired commerce transactions", body = CommerceRecentResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 500, description = "Query failed"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn commerce_recent<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    user: AuthenticatedUser,
    Query(params): Query<CommerceRecentParams>,
) -> impl IntoResponse {
    // Cap aggressively — this is a "recent" view, not a forensic dump.
    let limit = params.limit.clamp(1, 500);

    // Pull the user's recent events of any type. Commerce ones get
    // filtered in-process; others get dropped. We over-fetch by ~10x
    // because commerce events are rare per-user and we want a useful
    // window even if the trailing 100 raw events are all join_pu.
    let pull_limit: i64 = (limit as i64).saturating_mul(10).clamp(200, 1000);
    let filters = EventFilters {
        cursor: None,
        event_type: None,
        since: None,
        until: None,
        limit: pull_limit,
    };

    let events = match query.list_filtered(&user.preferred_username, filters).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "commerce_recent list_filtered failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "query_failed");
        }
    };

    // Deserialise each payload as GameEvent. Drop any that fail (the
    // store may hold legacy or malformed rows from an earlier client).
    let game_events: Vec<starstats_core::GameEvent> = events
        .into_iter()
        .filter_map(|e| serde_json::from_value(e.payload).ok())
        .collect();

    let now = Utc::now().to_rfc3339();
    let txs = starstats_core::pair_transactions(&game_events, &now, params.window_secs);

    // Trim to the requested limit (newest first by started_at after
    // pair_transactions sorts ascending — reverse + take). Convert
    // each row to the utoipa-friendly DTO so the OpenAPI spec stays
    // canonical without forcing utoipa onto starstats-core.
    let trimmed: Vec<CommerceTransactionDto> = txs
        .into_iter()
        .rev()
        .take(limit as usize)
        .map(CommerceTransactionDto::from)
        .collect();

    (
        StatusCode::OK,
        Json(CommerceRecentResponse {
            transactions: trimmed,
        }),
    )
        .into_response()
}

/// Wire-format wrapper for the commerce endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct CommerceRecentResponse {
    /// Paired transactions, newest first by started_at.
    pub transactions: Vec<CommerceTransactionDto>,
}

/// Mirrors `starstats_core::Transaction` but in a utoipa-friendly
/// shape. Field-for-field identical at the JSON layer.
#[derive(Debug, Serialize, ToSchema)]
pub struct CommerceTransactionDto {
    pub kind: String,
    pub status: String,
    pub started_at: String,
    pub confirmed_at: Option<String>,
    pub shop_id: Option<String>,
    pub item: Option<String>,
    pub quantity: Option<f64>,
    pub raw_request: String,
    pub raw_response: Option<String>,
}

impl From<starstats_core::Transaction> for CommerceTransactionDto {
    fn from(t: starstats_core::Transaction) -> Self {
        Self {
            kind: serde_json::to_value(t.kind)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default(),
            status: serde_json::to_value(t.status)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default(),
            started_at: t.started_at,
            confirmed_at: t.confirmed_at,
            shop_id: t.shop_id,
            item: t.item,
            quantity: t.quantity,
            raw_request: t.raw_request,
            raw_response: t.raw_response,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::test_support::fresh_pair;
    use crate::auth::{AuthVerifier, TokenIssuer};
    use crate::repo::{test_support::MemoryQuery, StoredQueryEvent};
    use axum::body::to_bytes;
    use axum::http::Request;
    use axum::routing::get;
    use axum::{Extension, Router};
    use serde_json::json;
    use tower::ServiceExt;

    fn sign_token(issuer: &TokenIssuer, username: &str) -> String {
        issuer
            .sign_user(&format!("user-{username}"), username)
            .expect("sign user token")
    }

    fn router(query: Arc<MemoryQuery>, verifier: Arc<AuthVerifier>) -> Router {
        // SpiceDB extension is required by `summary`; tests run
        // without a configured client, so we inject a None-valued
        // Arc — the handler treats that as "skipped".
        let spicedb: Arc<Option<crate::spicedb::SpicedbClient>> = Arc::new(None);
        Router::new()
            .route("/v1/me/events", get(list_events::<MemoryQuery>))
            .route("/v1/me/summary", get(summary::<MemoryQuery>))
            .route("/v1/me/timeline", get(timeline::<MemoryQuery>))
            .route(
                "/v1/me/metrics/event-types",
                get(metrics_event_types::<MemoryQuery>),
            )
            .route(
                "/v1/me/metrics/sessions",
                get(metrics_sessions::<MemoryQuery>),
            )
            .route("/v1/me/ingest-history", get(ingest_history::<MemoryQuery>))
            .route(
                "/v1/me/location/current",
                get(location_current::<MemoryQuery>),
            )
            .route("/v1/me/stats/combat", get(stats_combat::<MemoryQuery>))
            .layer(Extension(verifier))
            .layer(Extension(spicedb))
            .with_state(query)
    }

    fn evt(seq: i64, handle: &str, ty: &str, ts: Option<DateTime<Utc>>) -> StoredQueryEvent {
        StoredQueryEvent {
            seq,
            claimed_handle: handle.into(),
            event_type: ty.into(),
            event_timestamp: ts,
            log_source: "live".into(),
            source_offset: 0,
            payload: json!({"type": ty}),
        }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        app: Router,
        uri: &str,
        token: &str,
    ) -> (StatusCode, T) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let parsed: T = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            panic!(
                "decode {}: {} (body={})",
                std::any::type_name::<T>(),
                e,
                String::from_utf8_lossy(&bytes)
            )
        });
        (status, parsed)
    }

    #[tokio::test]
    async fn lists_events_for_authenticated_handle() {
        let mq = Arc::new(MemoryQuery::new(vec![
            StoredQueryEvent {
                seq: 1,
                claimed_handle: "Alice".into(),
                event_type: "join_pu".into(),
                event_timestamp: None,
                log_source: "live".into(),
                source_offset: 0,
                payload: json!({"type":"join_pu"}),
            },
            StoredQueryEvent {
                seq: 2,
                claimed_handle: "Bob".into(),
                event_type: "join_pu".into(),
                event_timestamp: None,
                log_source: "live".into(),
                source_offset: 0,
                payload: json!({"type":"join_pu"}),
            },
            StoredQueryEvent {
                seq: 3,
                claimed_handle: "Alice".into(),
                event_type: "actor_death".into(),
                event_timestamp: None,
                log_source: "live".into(),
                source_offset: 0,
                payload: json!({"type":"actor_death"}),
            },
        ]));

        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let req = Request::builder()
            .method("GET")
            .uri("/v1/me/events")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let parsed: EventsListResponse = serde_json::from_slice(&bytes).unwrap();

        // Only Alice's two events.
        assert_eq!(parsed.events.len(), 2);
        assert!(parsed
            .events
            .iter()
            .all(|e| e.event_type != "join_pu" || e.payload["type"] == "join_pu"));
        assert_eq!(parsed.next_after, Some(3));
    }

    #[tokio::test]
    async fn summary_aggregates_by_type() {
        let mq = Arc::new(MemoryQuery::new(vec![
            StoredQueryEvent {
                seq: 1,
                claimed_handle: "Alice".into(),
                event_type: "join_pu".into(),
                event_timestamp: None,
                log_source: "live".into(),
                source_offset: 0,
                payload: json!({}),
            },
            StoredQueryEvent {
                seq: 2,
                claimed_handle: "Alice".into(),
                event_type: "join_pu".into(),
                event_timestamp: None,
                log_source: "live".into(),
                source_offset: 0,
                payload: json!({}),
            },
            StoredQueryEvent {
                seq: 3,
                claimed_handle: "Alice".into(),
                event_type: "actor_death".into(),
                event_timestamp: None,
                log_source: "live".into(),
                source_offset: 0,
                payload: json!({}),
            },
            StoredQueryEvent {
                seq: 4,
                claimed_handle: "Bob".into(),
                event_type: "join_pu".into(),
                event_timestamp: None,
                log_source: "live".into(),
                source_offset: 0,
                payload: json!({}),
            },
        ]));

        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let req = Request::builder()
            .method("GET")
            .uri("/v1/me/summary")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let parsed: SummaryResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.claimed_handle, "Alice");
        assert_eq!(parsed.total, 3);
        let join = parsed
            .by_type
            .iter()
            .find(|t| t.event_type == "join_pu")
            .unwrap();
        assert_eq!(join.count, 2);
    }

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[tokio::test]
    async fn list_events_filter_by_event_type_returns_only_matching() {
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "join_pu", None),
            evt(2, "Alice", "actor_death", None),
            evt(3, "Alice", "join_pu", None),
            evt(4, "Alice", "vehicle_destruction", None),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<EventsListResponse>(app, "/v1/me/events?event_type=join_pu", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed.events.len(), 2);
        assert!(parsed.events.iter().all(|e| e.event_type == "join_pu"));
    }

    #[tokio::test]
    async fn list_events_filter_by_since_returns_events_after_timestamp() {
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", Some(ts("2026-04-01T00:00:00Z"))),
            evt(2, "Alice", "x", Some(ts("2026-04-15T00:00:00Z"))),
            evt(3, "Alice", "x", Some(ts("2026-05-01T00:00:00Z"))),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<EventsListResponse>(app, "/v1/me/events?since=2026-04-15T00:00:00Z", &token)
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed.events.len(), 2);
        let seqs: Vec<i64> = parsed.events.iter().map(|e| e.seq).collect();
        assert!(seqs.contains(&2));
        assert!(seqs.contains(&3));
    }

    #[tokio::test]
    async fn list_events_filter_by_until_returns_events_before_timestamp() {
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", Some(ts("2026-04-01T00:00:00Z"))),
            evt(2, "Alice", "x", Some(ts("2026-04-15T00:00:00Z"))),
            evt(3, "Alice", "x", Some(ts("2026-05-01T00:00:00Z"))),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<EventsListResponse>(app, "/v1/me/events?until=2026-04-15T00:00:00Z", &token)
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed.events.len(), 2);
        let seqs: Vec<i64> = parsed.events.iter().map(|e| e.seq).collect();
        assert!(seqs.contains(&1));
        assert!(seqs.contains(&2));
    }

    #[tokio::test]
    async fn list_events_before_seq_paginates_descending() {
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", None),
            evt(2, "Alice", "x", None),
            evt(3, "Alice", "x", None),
            evt(4, "Alice", "x", None),
            evt(5, "Alice", "x", None),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<EventsListResponse>(app, "/v1/me/events?before_seq=4&limit=2", &token).await;
        assert_eq!(status, StatusCode::OK);
        let seqs: Vec<i64> = parsed.events.iter().map(|e| e.seq).collect();
        // Strictly less than 4, DESC, limit 2 -> [3, 2].
        assert_eq!(seqs, vec![3, 2]);
    }

    #[tokio::test]
    async fn list_events_after_seq_paginates_ascending() {
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", None),
            evt(2, "Alice", "x", None),
            evt(3, "Alice", "x", None),
            evt(4, "Alice", "x", None),
            evt(5, "Alice", "x", None),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<EventsListResponse>(app, "/v1/me/events?after_seq=2&limit=2", &token).await;
        assert_eq!(status, StatusCode::OK);
        let seqs: Vec<i64> = parsed.events.iter().map(|e| e.seq).collect();
        // Strictly greater than 2, ASC, limit 2 -> [3, 4].
        assert_eq!(seqs, vec![3, 4]);
    }

    #[tokio::test]
    async fn list_events_rejects_both_cursors() {
        let mq = Arc::new(MemoryQuery::new(vec![]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let req = Request::builder()
            .method("GET")
            .uri("/v1/me/events?before_seq=10&after_seq=2")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let body: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.error, "conflicting_cursors");
    }

    #[tokio::test]
    async fn list_events_rejects_invalid_event_type() {
        let mq = Arc::new(MemoryQuery::new(vec![]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        // Uppercase + dash -> invalid by [a-z0-9_]{1,64} rule.
        let req = Request::builder()
            .method("GET")
            .uri("/v1/me/events?event_type=Join-PU")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let body: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.error, "invalid_event_type");
    }

    #[tokio::test]
    async fn timeline_returns_per_day_counts_with_zero_padding() {
        let now = Utc::now();
        let one_day_ago = now - Duration::days(1);
        let three_days_ago = now - Duration::days(3);
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", Some(now)),
            evt(2, "Alice", "x", Some(now)),
            evt(3, "Alice", "x", Some(one_day_ago)),
            evt(4, "Alice", "x", Some(three_days_ago)),
            // Bob's events should be excluded.
            evt(5, "Bob", "x", Some(now)),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<TimelineResponse>(app, "/v1/me/timeline?days=7", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed.days, 7);
        // Zero-padded to exactly `days` buckets.
        assert_eq!(parsed.buckets.len(), 7);

        let total: u64 = parsed.buckets.iter().map(|b| b.count).sum();
        // Alice has 4 events inside the window (now, now, -1d, -3d).
        assert_eq!(total, 4);

        let today_key = now.date_naive().format("%Y-%m-%d").to_string();
        let today_bucket = parsed
            .buckets
            .iter()
            .find(|b| b.date == today_key)
            .expect("today bucket present");
        assert_eq!(today_bucket.count, 2);

        // At least one bucket is zero — that's the zero-padding.
        assert!(parsed.buckets.iter().any(|b| b.count == 0));

        // Buckets must be ordered ascending by date.
        let dates: Vec<&String> = parsed.buckets.iter().map(|b| &b.date).collect();
        let mut sorted = dates.clone();
        sorted.sort();
        assert_eq!(dates, sorted);
    }

    #[tokio::test]
    async fn timeline_rejects_days_above_max() {
        let mq = Arc::new(MemoryQuery::new(vec![]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let req = Request::builder()
            .method("GET")
            .uri("/v1/me/timeline?days=365")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let body: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.error, "invalid_days");
    }

    #[tokio::test]
    async fn metrics_event_types_returns_count_and_last_seen() {
        let now = Utc::now();
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "join_pu", Some(now - Duration::days(40))),
            evt(2, "Alice", "join_pu", Some(now - Duration::days(2))),
            evt(3, "Alice", "actor_death", Some(now - Duration::hours(1))),
            evt(4, "Bob", "join_pu", Some(now)),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) = get_json::<EventTypeBreakdownResponse>(
            app,
            "/v1/me/metrics/event-types?range=30d",
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed.range, "30d");
        // Within the 30-day window: 1 join_pu + 1 actor_death.
        // Bob's row excluded.
        let join = parsed
            .types
            .iter()
            .find(|t| t.event_type == "join_pu")
            .expect("join_pu present");
        assert_eq!(join.count, 1);
        let death = parsed
            .types
            .iter()
            .find(|t| t.event_type == "actor_death")
            .expect("actor_death present");
        assert_eq!(death.count, 1);
        assert!(death.last_seen.is_some());
    }

    #[tokio::test]
    async fn metrics_event_types_range_all_includes_old_rows() {
        let now = Utc::now();
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "join_pu", Some(now - Duration::days(400))),
            evt(2, "Alice", "join_pu", Some(now - Duration::days(2))),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) = get_json::<EventTypeBreakdownResponse>(
            app,
            "/v1/me/metrics/event-types?range=all",
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed.range, "all");
        let join = parsed
            .types
            .iter()
            .find(|t| t.event_type == "join_pu")
            .unwrap();
        // No range filter -> both Alice's rows count.
        assert_eq!(join.count, 2);
    }

    #[tokio::test]
    async fn metrics_event_types_rejects_unknown_range() {
        let mq = Arc::new(MemoryQuery::new(vec![]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let req = Request::builder()
            .method("GET")
            .uri("/v1/me/metrics/event-types?range=year")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let body: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.error, "invalid_range");
    }

    #[tokio::test]
    async fn metrics_sessions_groups_events_within_idle_threshold() {
        // Three events 5 minutes apart -> 1 session of 3 events.
        // Then a 60-minute gap -> new session with 2 more events.
        let base = ts("2026-04-15T10:00:00Z");
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", Some(base)),
            evt(2, "Alice", "x", Some(base + Duration::minutes(5))),
            evt(3, "Alice", "x", Some(base + Duration::minutes(10))),
            evt(4, "Alice", "x", Some(base + Duration::minutes(70))),
            evt(5, "Alice", "x", Some(base + Duration::minutes(80))),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<SessionsResponse>(app, "/v1/me/metrics/sessions", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed.sessions.len(), 2);

        // Newest first.
        assert_eq!(parsed.sessions[0].event_count, 2);
        assert_eq!(parsed.sessions[0].start_at, base + Duration::minutes(70));
        assert_eq!(parsed.sessions[0].end_at, base + Duration::minutes(80));

        assert_eq!(parsed.sessions[1].event_count, 3);
        assert_eq!(parsed.sessions[1].start_at, base);
        assert_eq!(parsed.sessions[1].end_at, base + Duration::minutes(10));
    }

    #[tokio::test]
    async fn metrics_sessions_excludes_other_handles() {
        let base = ts("2026-04-15T10:00:00Z");
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", Some(base)),
            evt(2, "Bob", "x", Some(base + Duration::minutes(5))),
            evt(3, "Alice", "x", Some(base + Duration::minutes(8))),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<SessionsResponse>(app, "/v1/me/metrics/sessions", &token).await;
        assert_eq!(status, StatusCode::OK);
        // Only Alice's two events; one session of 2.
        assert_eq!(parsed.sessions.len(), 1);
        assert_eq!(parsed.sessions[0].event_count, 2);
    }

    fn batch(seq: i64, occurred_at: DateTime<Utc>, total: i64, accepted: i64) -> IngestBatchRow {
        IngestBatchRow {
            seq,
            occurred_at,
            batch_id: format!("b{seq}"),
            game_build: Some("4.0-LIVE.test".into()),
            total,
            accepted,
            duplicate: 0,
            rejected: total - accepted,
        }
    }

    #[tokio::test]
    async fn ingest_history_returns_calling_handles_batches_newest_first() {
        let now = Utc::now();
        let mq = Arc::new(MemoryQuery::new(vec![]).with_ingest_history(vec![
            (
                "Alice".into(),
                batch(10, now - Duration::hours(1), 200, 198),
            ),
            (
                "Alice".into(),
                batch(11, now - Duration::minutes(20), 50, 50),
            ),
            ("Bob".into(), batch(12, now - Duration::minutes(5), 30, 30)),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) =
            get_json::<IngestHistoryResponse>(app, "/v1/me/ingest-history", &token).await;
        assert_eq!(status, StatusCode::OK);
        // Bob's row excluded, Alice's two rows newest first.
        assert_eq!(parsed.batches.len(), 2);
        assert_eq!(parsed.batches[0].seq, 11);
        assert_eq!(parsed.batches[1].seq, 10);
        assert_eq!(parsed.batches[0].total, 50);
        assert_eq!(parsed.batches[1].accepted, 198);
    }

    #[tokio::test]
    async fn ingest_history_paginates_via_offset() {
        let now = Utc::now();
        let mq = Arc::new(MemoryQuery::new(vec![]).with_ingest_history(vec![
            ("Alice".into(), batch(1, now - Duration::hours(3), 10, 10)),
            ("Alice".into(), batch(2, now - Duration::hours(2), 10, 10)),
            ("Alice".into(), batch(3, now - Duration::hours(1), 10, 10)),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, parsed) = get_json::<IngestHistoryResponse>(
            app,
            "/v1/me/ingest-history?limit=2&offset=1",
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // Newest-first ordering means seq 3,2,1 -> offset 1 limit 2 -> [2,1].
        assert_eq!(parsed.batches.len(), 2);
        assert_eq!(parsed.batches[0].seq, 2);
        assert_eq!(parsed.batches[1].seq, 1);
    }

    #[tokio::test]
    async fn metrics_sessions_paginates_via_offset() {
        let base = ts("2026-04-15T10:00:00Z");
        // Three sessions, each separated by > 30 min.
        let mq = Arc::new(MemoryQuery::new(vec![
            evt(1, "Alice", "x", Some(base)),
            evt(2, "Alice", "x", Some(base + Duration::hours(2))),
            evt(3, "Alice", "x", Some(base + Duration::hours(4))),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq.clone(), Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");

        let (status, page1) =
            get_json::<SessionsResponse>(app, "/v1/me/metrics/sessions?limit=2&offset=0", &token)
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page1.sessions.len(), 2);

        let (issuer2, verifier2) = fresh_pair();
        let app2 = router(mq, Arc::new(verifier2));
        let token2 = sign_token(&issuer2, "Alice");
        let (_, page2) =
            get_json::<SessionsResponse>(app2, "/v1/me/metrics/sessions?limit=2&offset=2", &token2)
                .await;
        assert_eq!(page2.sessions.len(), 1);
    }

    // -- /v1/me/location/current ---------------------------------

    fn evt_with_payload(
        seq: i64,
        handle: &str,
        ty: &str,
        ts: DateTime<Utc>,
        payload: serde_json::Value,
    ) -> StoredQueryEvent {
        StoredQueryEvent {
            seq,
            claimed_handle: handle.into(),
            event_type: ty.into(),
            event_timestamp: Some(ts),
            log_source: "live".into(),
            source_offset: 0,
            payload,
        }
    }

    async fn get_status_and_bytes(app: Router, uri: &str, token: &str) -> (StatusCode, Vec<u8>) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap().to_vec();
        (status, bytes)
    }

    #[tokio::test]
    async fn location_current_returns_204_when_user_has_no_events() {
        let mq = Arc::new(MemoryQuery::new(vec![]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");
        let (status, _) = get_status_and_bytes(app, "/v1/me/location/current", &token).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn location_current_returns_204_when_latest_event_is_stale() {
        // 2 hours old — outside the 90-minute window.
        let stale = Utc::now() - Duration::hours(2);
        let mq = Arc::new(MemoryQuery::new(vec![evt_with_payload(
            1,
            "Alice",
            "planet_terrain_load",
            stale,
            json!({"planet": "OOC_Stanton_2b_Daymar"}),
        )]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");
        let (status, _) = get_status_and_bytes(app, "/v1/me/location/current", &token).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn location_current_resolves_planet_terrain_within_window() {
        let recent = Utc::now() - Duration::minutes(5);
        let mq = Arc::new(MemoryQuery::new(vec![evt_with_payload(
            1,
            "Alice",
            "planet_terrain_load",
            recent,
            json!({"planet": "OOC_Stanton_2b_Daymar"}),
        )]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");
        let (status, body) =
            get_json::<CurrentLocationResponse>(app, "/v1/me/location/current", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.location.planet.as_deref(), Some("Daymar"));
        assert_eq!(body.location.system.as_deref(), Some("Stanton"));
        assert_eq!(body.location.source_event_type, "planet_terrain_load");
    }

    #[tokio::test]
    async fn location_current_prefers_most_recent_event_over_older_more_precise_one() {
        // Older inventory request (precise: city) followed by newer
        // planet_terrain (less precise: planet only). The handler
        // surfaces the most-recent reading, NOT the most-precise one
        // — staleness is the dominant axis. The precise reading might
        // mis-represent where the user is RIGHT NOW.
        let now = Utc::now();
        let mq = Arc::new(MemoryQuery::new(vec![
            evt_with_payload(
                1,
                "Alice",
                "location_inventory_requested",
                now - Duration::minutes(60),
                json!({"location": "Stanton1_Lorville"}),
            ),
            evt_with_payload(
                2,
                "Alice",
                "planet_terrain_load",
                now - Duration::minutes(2),
                json!({"planet": "OOC_Stanton_2b_Daymar"}),
            ),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");
        let (status, body) =
            get_json::<CurrentLocationResponse>(app, "/v1/me/location/current", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.location.planet.as_deref(), Some("Daymar"));
        assert!(
            body.location.city.is_none(),
            "city should be None — the recent event was planet-only"
        );
    }

    #[tokio::test]
    async fn location_current_attaches_shard_hint_from_separate_join_pu() {
        let now = Utc::now();
        let mq = Arc::new(MemoryQuery::new(vec![
            evt_with_payload(
                1,
                "Alice",
                "join_pu",
                now - Duration::minutes(30),
                json!({"shard": "pub_euw1b_test", "address": "1.2.3.4", "port": 64300, "location_id": "1"}),
            ),
            evt_with_payload(
                2,
                "Alice",
                "planet_terrain_load",
                now - Duration::minutes(2),
                json!({"planet": "OOC_Stanton_1_Hurston"}),
            ),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");
        let (status, body) =
            get_json::<CurrentLocationResponse>(app, "/v1/me/location/current", &token).await;
        assert_eq!(status, StatusCode::OK);
        // Headline is the planet from the recent event…
        assert_eq!(body.location.planet.as_deref(), Some("Hurston"));
        // …with the shard from the older join_pu carried as context.
        assert_eq!(body.location.shard.as_deref(), Some("pub_euw1b_test"));
    }

    #[tokio::test]
    async fn stats_combat_splits_kills_and_deaths_via_payload_filter() {
        // Three actor_death events in the user's stream:
        //   1. Caller killed npc_pirate (caller is killer)
        //   2. npc_pirate killed caller (caller is victim)
        //   3. Two npcs fighting each other (caller is neither)
        // Expected: kills=1, deaths=1. The third row inflates the
        // pre-fix "total deaths" count but is correctly excluded by
        // both filters.
        let now = Utc::now() - Duration::hours(1);
        let mq = Arc::new(MemoryQuery::new(vec![
            evt_with_payload(
                1,
                "Alice",
                "actor_death",
                now,
                json!({
                    "killer": "Alice",
                    "victim": "npc_pirate",
                    "weapon": "P4AR",
                    "zone": "ArcCorp",
                    "damage_type": "Bullet"
                }),
            ),
            evt_with_payload(
                2,
                "Alice",
                "actor_death",
                now + Duration::minutes(5),
                json!({
                    "killer": "npc_pirate",
                    "victim": "Alice",
                    "weapon": "S71",
                    "zone": "Daymar",
                    "damage_type": "Bullet"
                }),
            ),
            evt_with_payload(
                3,
                "Alice",
                "actor_death",
                now + Duration::minutes(10),
                json!({
                    "killer": "npc_a",
                    "victim": "npc_b",
                    "weapon": "Knife",
                    "zone": "Lorville",
                    "damage_type": "Melee"
                }),
            ),
        ]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");
        let (status, body) =
            get_json::<CombatStatsResponse>(app, "/v1/me/stats/combat?hours=24", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.kills, 1, "kills counts only killer==Alice rows");
        assert_eq!(body.deaths, 1, "deaths counts only victim==Alice rows");
        // Top weapons scoped to kills — only the P4AR (used by Alice)
        // should appear, NOT the S71 (used to kill Alice) or Knife
        // (NPC vs NPC).
        assert_eq!(body.top_weapons.len(), 1);
        assert_eq!(body.top_weapons[0].value, "P4AR");
        // Hot zones scoped to deaths — only Daymar (where Alice died)
        // should appear, NOT ArcCorp (where Alice killed) or Lorville
        // (NPC vs NPC).
        assert_eq!(body.deaths_by_zone.len(), 1);
        assert_eq!(body.deaths_by_zone[0].value, "Daymar");
    }

    #[tokio::test]
    async fn location_current_scopes_by_authenticated_handle() {
        // Bob is paired and online; Alice has no events. The endpoint
        // must NOT leak Bob's location to Alice.
        let now = Utc::now();
        let mq = Arc::new(MemoryQuery::new(vec![evt_with_payload(
            1,
            "Bob",
            "planet_terrain_load",
            now - Duration::minutes(2),
            json!({"planet": "OOC_Stanton_2b_Daymar"}),
        )]));
        let (issuer, verifier) = fresh_pair();
        let app = router(mq, Arc::new(verifier));
        let token = sign_token(&issuer, "Alice");
        let (status, _) = get_status_and_bytes(app, "/v1/me/location/current", &token).await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "Alice must not see Bob's location"
        );
    }
}
