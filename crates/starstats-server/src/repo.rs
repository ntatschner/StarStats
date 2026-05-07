//! Storage abstraction for ingested events.
//!
//! The handler depends on the [`EventStore`] trait, not on Postgres
//! directly, so we can TDD against an in-memory implementation.
//! Production wiring uses [`PostgresStore`].

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgPool;
use starstats_core::wire::{EventEnvelope, LogSource};
#[cfg(test)]
use std::sync::Mutex;
use uuid::Uuid;

/// What the server actually stores. Constructed from an
/// [`EventEnvelope`] plus the authenticated identity (claimed handle
/// only for now — `user_id` lands when auth does).
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub id: Uuid,
    pub idempotency_key: String,
    pub claimed_handle: String,
    pub event_type: String,
    pub event_timestamp: Option<DateTime<Utc>>,
    pub log_source: LogSource,
    pub source_offset: i64,
    pub raw_line: String,
    pub payload: Value,
}

/// Outcome of inserting one event. Lets the handler report how many
/// were accepted vs deduped vs rejected without separate round-trips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    Duplicate,
}

#[async_trait]
pub trait EventStore: Send + Sync + 'static {
    async fn insert(&self, event: StoredEvent) -> Result<InsertOutcome, RepoError>;
}

/// Read-side projection of an event row. Subset of the storage table
/// — `raw_line` and `idempotency_key` aren't surfaced by query
/// endpoints (clients have their own copies; the raw line is only
/// useful for re-classification by the server).
#[derive(Debug, Clone)]
pub struct StoredQueryEvent {
    pub seq: i64,
    /// Used by query filters; not surfaced to the API DTO since
    /// the caller already knows their own handle.
    #[allow(dead_code)]
    pub claimed_handle: String,
    pub event_type: String,
    pub event_timestamp: Option<DateTime<Utc>>,
    pub log_source: String,
    pub source_offset: i64,
    pub payload: Value,
}

/// Direction of cursor pagination on `event_seq`.
///
/// `Before` returns rows older than the cursor in DESC order (the
/// default "newest first" stream walking backwards). `After` returns
/// rows newer than the cursor in ASC order (catch-up tailing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqCursor {
    Before(i64),
    After(i64),
}

/// Filter / pagination spec for [`EventQuery::list_filtered`]. All
/// fields except `limit` are optional and compose with AND semantics.
///
/// Cursor semantics:
///  * `None` -> newest-first (DESC by seq).
///  * `Some(SeqCursor::Before(n))` -> rows with seq < n, DESC by seq.
///  * `Some(SeqCursor::After(n))`  -> rows with seq > n, ASC by seq.
#[derive(Debug, Clone, Default)]
pub struct EventFilters {
    pub cursor: Option<SeqCursor>,
    pub event_type: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: i64,
}

/// Idle gap (in minutes) between two adjacent events that splits a
/// session boundary. Tuned by hand: a typical Star Citizen session has
/// events every few seconds; a 30-minute lull is "they alt-tabbed for a
/// snack and came back" being generous, while still splitting actual
/// distinct play sessions cleanly.
pub const SESSION_IDLE_GAP_MINUTES: i64 = 30;

/// One inferred play session — a contiguous run of events where no
/// adjacent pair is more than [`SESSION_IDLE_GAP_MINUTES`] apart.
/// Computed on demand from the events table; not persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredSession {
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub event_count: i64,
}

/// One row of the `event-types` aggregate. Mirrors
/// [`crate::query::TypeCount`] plus a `last_seen` timestamp; emitted
/// by [`EventQuery::event_type_breakdown`] which is the back-end of the
/// Metrics page's "Event types" tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTypeStats {
    pub event_type: String,
    pub count: i64,
    /// `None` for types whose only rows had no parsed timestamp. The
    /// table column is `event_timestamp`, which is nullable.
    pub last_seen: Option<DateTime<Utc>>,
}

/// One row of the `ingest-history` view: a single batch the caller's
/// desktop client posted. Read straight off `audit_log` filtered to
/// `action = 'ingest.batch_processed'` for the authenticated handle.
/// We deliberately don't retain the raw lines from the batch, so this
/// is metadata only: who shipped what, when, and what the server's
/// per-event accept/dup/reject verdict was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestBatchRow {
    pub seq: i64,
    pub occurred_at: DateTime<Utc>,
    pub batch_id: String,
    pub game_build: Option<String>,
    pub total: i64,
    pub accepted: i64,
    pub duplicate: i64,
    pub rejected: i64,
}

#[async_trait]
pub trait EventQuery: Send + Sync + 'static {
    /// Legacy forward-cursor listing. Kept on the trait so external
    /// callers (and the legacy `?after=` query param path) can still
    /// reach it; the new handler always goes through
    /// [`Self::list_filtered`].
    #[allow(dead_code)]
    async fn list_for_handle(
        &self,
        claimed_handle: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<StoredQueryEvent>, RepoError> {
        self.list_filtered(
            claimed_handle,
            EventFilters {
                cursor: if after > 0 {
                    Some(SeqCursor::After(after))
                } else {
                    None
                },
                event_type: None,
                since: None,
                until: None,
                limit,
            },
        )
        .await
    }

    /// Filtered + cursor-paginated listing. Composes optional
    /// `event_type`, `since`, `until`, and a single seq cursor.
    async fn list_filtered(
        &self,
        claimed_handle: &str,
        filters: EventFilters,
    ) -> Result<Vec<StoredQueryEvent>, RepoError>;

    /// Per-day event counts for the trailing `days` window. Returns
    /// only days that had events; the handler is responsible for
    /// zero-padding the bucket series.
    async fn timeline(
        &self,
        claimed_handle: &str,
        days: u32,
    ) -> Result<Vec<(NaiveDate, i64)>, RepoError>;

    /// Returns (total, [(event_type, count)]).
    async fn summary_for_handle(
        &self,
        claimed_handle: &str,
    ) -> Result<(u64, Vec<(String, u64)>), RepoError>;

    /// Per-event-type breakdown with `last_seen` for the Metrics page's
    /// "Event types" tab. Returns rows sorted by count DESC. If
    /// `since` is set, only events with `event_timestamp >= since` are
    /// counted; rows whose only matches all had NULL timestamps are
    /// dropped (we can't show "last seen" for them and they aren't
    /// actionable on the time-windowed view).
    async fn event_type_breakdown(
        &self,
        claimed_handle: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<EventTypeStats>, RepoError>;

    /// Inferred play sessions for the Metrics page's "Sessions" tab.
    /// Groups consecutive events by `event_timestamp` and starts a new
    /// session whenever the gap between two adjacent events exceeds
    /// [`SESSION_IDLE_GAP_MINUTES`]. Events with NULL `event_timestamp`
    /// are excluded — they can't anchor a session window. Returns rows
    /// newest-first; the handler exposes `limit`/`offset` pagination.
    async fn sessions_for_handle(
        &self,
        claimed_handle: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<InferredSession>, RepoError>;

    /// Recent ingest batches the caller's clients posted, newest first.
    /// Backs the My logs page (Wave 11). Reads `audit_log` filtered to
    /// the canonical ingest action; the user's desktop client is the
    /// only writer of those rows for that handle, so cross-account
    /// leakage is prevented by the `actor_handle = $1` filter alone.
    async fn ingest_history_for_handle(
        &self,
        actor_handle: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IngestBatchRow>, RepoError>;

    /// Returns the most recent location-bearing event for the user
    /// alongside the most recent `join_pu` shard hint, if any.
    /// Backs `GET /v1/me/location/current`.
    ///
    /// Caller passes the list of acceptable event types (canonical
    /// list lives in [`crate::locations::LOCATION_EVENT_TYPES`]) and
    /// gets back two independent reads:
    ///
    /// - `location_event` — the most recent event whose type is in
    ///   the list. The handler funnels its payload through
    ///   [`crate::locations::resolve`] to produce the wire DTO.
    /// - `shard_hint` — the shard string from the most recent
    ///   `join_pu` event regardless of where in the result list it
    ///   sits. Lets the resolver attach shard info even when a more
    ///   recent `planet_terrain_load` is the headline event.
    async fn latest_location(
        &self,
        claimed_handle: &str,
        event_types: &[&str],
    ) -> Result<LatestLocation, RepoError>;

    /// Aggregate dwell time per `(planet, city)` pair over a window.
    /// Backs `GET /v1/me/location/breakdown`. Returns rows with
    /// `dwell_seconds` derived from the gap between adjacent
    /// location events (capped at the session-idle threshold to
    /// avoid an idle gap inflating one location's dwell). Sorted by
    /// dwell DESC.
    ///
    /// Implementation note: this returns the raw event stream in the
    /// query window — the handler does the gap-walk + dedup +
    /// labelling using [`crate::locations::resolve`]. Keeping it on
    /// the handler side means the dwell aggregation logic stays
    /// pure-Rust (testable without a database) and the repo stays a
    /// thin SQL layer.
    async fn location_event_stream(
        &self,
        claimed_handle: &str,
        event_types: &[&str],
        since: DateTime<Utc>,
    ) -> Result<Vec<LatestLocationEvent>, RepoError>;

    /// Aggregate stats from the events table for the activity
    /// surface. Returns counts grouped by a JSON-payload field for
    /// the named event_type, sorted by count DESC.
    ///
    /// `payload_field` is a top-level JSON key (no dotted paths).
    /// `since` filters to events after this instant. `payload_filter`,
    /// when set, restricts to rows whose given field equals the
    /// expected value (case-sensitive). Used by the combat stats
    /// handler to scope "top weapons" to kills (killer==caller) vs
    /// "deaths by zone" to deaths (victim==caller).
    async fn payload_field_breakdown(
        &self,
        claimed_handle: &str,
        event_type: &str,
        payload_field: &str,
        payload_filter: Option<PayloadFilter<'_>>,
        since: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<Vec<PayloadFieldBucket>, RepoError>;

    /// Total count of events of the given type for the user, in the
    /// optional `since` window. `payload_filter` lets the caller
    /// scope to rows whose given JSON field equals the expected value
    /// — that's how `stats_combat` separates kills (killer==caller)
    /// from deaths (victim==caller) using the same `actor_death`
    /// table.
    async fn count_event_type(
        &self,
        claimed_handle: &str,
        event_type: &str,
        payload_filter: Option<PayloadFilter<'_>>,
        since: Option<DateTime<Utc>>,
    ) -> Result<u64, RepoError>;
}

/// Filter clause for the activity-stats queries. Both methods that
/// take `Option<PayloadFilter<'_>>` apply it as a `payload->>field =
/// value` predicate on the `events` table. Borrowed form so the
/// caller doesn't have to allocate.
#[derive(Debug, Clone, Copy)]
pub struct PayloadFilter<'a> {
    pub field: &'a str,
    pub equals: &'a str,
}

/// One bucket from [`EventQuery::payload_field_breakdown`].
#[derive(Debug, Clone)]
pub struct PayloadFieldBucket {
    pub value: String,
    pub count: i64,
}

/// What [`EventQuery::latest_location`] returns. Both fields are
/// independently optional so the handler can distinguish "no events
/// at all yet" (both `None` → 204) from "we know they're online but
/// don't know where" (only `shard_hint` populated → return the
/// `JoinPu`-shaped fallback).
#[derive(Debug, Clone, Default)]
pub struct LatestLocation {
    pub location_event: Option<LatestLocationEvent>,
    pub shard_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LatestLocationEvent {
    pub event_type: String,
    pub event_timestamp: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Build a [`StoredEvent`] from the wire envelope plus the
/// authenticated handle. Pulls `event_type` and `event_timestamp` out
/// of the parsed payload when present, falling back to `"unknown"`
/// for unclassified lines so we can still insert them.
///
/// If `serde_json::to_value` ever fails (custom Serialize impl, NaN
/// float, etc.) we still insert the row with `payload: Null` so the
/// idempotency key is recorded — but we log loudly with the
/// idempotency key + raw line preview so the operator notices.
/// Without the warning the row sits in the DB as
/// `event_type=unknown, payload=null` indistinguishable from a real
/// unparseable line, and the bug stays buried.
pub fn from_envelope(env: &EventEnvelope, claimed_handle: &str) -> StoredEvent {
    let payload = match serde_json::to_value(&env.event) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = %e,
                idempotency_key = %env.idempotency_key,
                claimed_handle = %claimed_handle,
                source_offset = env.source_offset,
                "envelope event payload failed to serialize; storing as null"
            );
            Value::Null
        }
    };
    let (event_type, event_timestamp) = extract_type_and_ts(&payload);
    StoredEvent {
        id: Uuid::now_v7(),
        idempotency_key: env.idempotency_key.clone(),
        claimed_handle: claimed_handle.to_owned(),
        event_type,
        event_timestamp,
        log_source: env.source,
        source_offset: env.source_offset as i64,
        raw_line: env.raw_line.clone(),
        payload,
    }
}

fn extract_type_and_ts(payload: &Value) -> (String, Option<DateTime<Utc>>) {
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let timestamp = payload
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    (event_type, timestamp)
}

// -- Test-only query stub --------------------------------------------

#[cfg(test)]
pub mod test_support {
    use super::*;

    pub struct MemoryQuery {
        rows: Vec<StoredQueryEvent>,
        /// (actor_handle, row) pairs. The production read path filters
        /// audit_log by `actor_handle`, so the test stub keeps the
        /// handle alongside each row to mirror that scoping.
        ingest_history: Vec<(String, IngestBatchRow)>,
    }

    impl MemoryQuery {
        pub fn new(rows: Vec<StoredQueryEvent>) -> Self {
            Self {
                rows,
                ingest_history: Vec::new(),
            }
        }

        pub fn with_ingest_history(mut self, history: Vec<(String, IngestBatchRow)>) -> Self {
            self.ingest_history = history;
            self
        }
    }

    #[async_trait]
    impl EventQuery for MemoryQuery {
        async fn list_filtered(
            &self,
            claimed_handle: &str,
            filters: EventFilters,
        ) -> Result<Vec<StoredQueryEvent>, RepoError> {
            let mut rows: Vec<StoredQueryEvent> = self
                .rows
                .iter()
                .filter(|r| r.claimed_handle == claimed_handle)
                .filter(|r| match &filters.event_type {
                    Some(t) => &r.event_type == t,
                    None => true,
                })
                .filter(|r| match (filters.since, r.event_timestamp) {
                    (Some(s), Some(ts)) => ts >= s,
                    (Some(_), None) => false,
                    _ => true,
                })
                .filter(|r| match (filters.until, r.event_timestamp) {
                    (Some(u), Some(ts)) => ts <= u,
                    (Some(_), None) => false,
                    _ => true,
                })
                .filter(|r| match filters.cursor {
                    Some(SeqCursor::Before(n)) => r.seq < n,
                    Some(SeqCursor::After(n)) => r.seq > n,
                    None => true,
                })
                .cloned()
                .collect();

            match filters.cursor {
                Some(SeqCursor::After(_)) => rows.sort_by_key(|r| r.seq),
                _ => rows.sort_by(|a, b| b.seq.cmp(&a.seq)),
            }

            rows.truncate(filters.limit.max(0) as usize);
            Ok(rows)
        }

        async fn timeline(
            &self,
            claimed_handle: &str,
            days: u32,
        ) -> Result<Vec<(NaiveDate, i64)>, RepoError> {
            // Window: events whose event_timestamp is within the last
            // `days` days. The handler does the zero-padding pass; we
            // only return present days.
            let since = Utc::now() - chrono::Duration::days(days as i64);
            let mut counts: std::collections::BTreeMap<NaiveDate, i64> =
                std::collections::BTreeMap::new();
            for r in &self.rows {
                if r.claimed_handle != claimed_handle {
                    continue;
                }
                let Some(ts) = r.event_timestamp else {
                    continue;
                };
                if ts < since {
                    continue;
                }
                *counts.entry(ts.date_naive()).or_default() += 1;
            }
            Ok(counts.into_iter().collect())
        }

        async fn summary_for_handle(
            &self,
            claimed_handle: &str,
        ) -> Result<(u64, Vec<(String, u64)>), RepoError> {
            let mine: Vec<&StoredQueryEvent> = self
                .rows
                .iter()
                .filter(|r| r.claimed_handle == claimed_handle)
                .collect();
            let total = mine.len() as u64;
            let mut counts: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            for e in &mine {
                *counts.entry(e.event_type.clone()).or_default() += 1;
            }
            let mut by_type: Vec<(String, u64)> = counts.into_iter().collect();
            by_type.sort_by(|a, b| b.1.cmp(&a.1));
            Ok((total, by_type))
        }

        async fn event_type_breakdown(
            &self,
            claimed_handle: &str,
            since: Option<DateTime<Utc>>,
        ) -> Result<Vec<EventTypeStats>, RepoError> {
            let mut counts: std::collections::HashMap<String, (i64, Option<DateTime<Utc>>)> =
                std::collections::HashMap::new();
            for r in &self.rows {
                if r.claimed_handle != claimed_handle {
                    continue;
                }
                if let (Some(s), Some(ts)) = (since, r.event_timestamp) {
                    if ts < s {
                        continue;
                    }
                } else if since.is_some() && r.event_timestamp.is_none() {
                    // The Postgres impl drops timestampless rows when
                    // `since` is set — match it so tests are honest.
                    continue;
                }
                let entry = counts.entry(r.event_type.clone()).or_insert((0, None));
                entry.0 += 1;
                if let Some(ts) = r.event_timestamp {
                    entry.1 = Some(entry.1.map_or(ts, |prev| prev.max(ts)));
                }
            }
            let mut rows: Vec<EventTypeStats> = counts
                .into_iter()
                .map(|(event_type, (count, last_seen))| EventTypeStats {
                    event_type,
                    count,
                    last_seen,
                })
                .collect();
            rows.sort_by(|a, b| {
                b.count
                    .cmp(&a.count)
                    .then_with(|| a.event_type.cmp(&b.event_type))
            });
            Ok(rows)
        }

        async fn sessions_for_handle(
            &self,
            claimed_handle: &str,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<InferredSession>, RepoError> {
            // Replicates the Postgres window-function logic in plain Rust:
            // sort the (timestampful) events ascending, walk them, and
            // start a new session whenever the gap to the previous event
            // exceeds the configured idle threshold.
            let mut timestamps: Vec<DateTime<Utc>> = self
                .rows
                .iter()
                .filter(|r| r.claimed_handle == claimed_handle)
                .filter_map(|r| r.event_timestamp)
                .collect();
            timestamps.sort();

            let gap = chrono::Duration::minutes(SESSION_IDLE_GAP_MINUTES);
            let mut sessions: Vec<InferredSession> = Vec::new();
            for ts in timestamps {
                match sessions.last_mut() {
                    Some(s) if ts - s.end_at <= gap => {
                        s.end_at = ts;
                        s.event_count += 1;
                    }
                    _ => sessions.push(InferredSession {
                        start_at: ts,
                        end_at: ts,
                        event_count: 1,
                    }),
                }
            }
            sessions.sort_by(|a, b| b.start_at.cmp(&a.start_at));
            let start = offset.max(0) as usize;
            let take = limit.max(0) as usize;
            Ok(sessions.into_iter().skip(start).take(take).collect())
        }

        async fn ingest_history_for_handle(
            &self,
            actor_handle: &str,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<IngestBatchRow>, RepoError> {
            let mut rows: Vec<IngestBatchRow> = self
                .ingest_history
                .iter()
                .filter(|(h, _)| h == actor_handle)
                .map(|(_, row)| row.clone())
                .collect();
            rows.sort_by(|a, b| b.seq.cmp(&a.seq));
            let start = offset.max(0) as usize;
            let take = limit.max(0) as usize;
            Ok(rows.into_iter().skip(start).take(take).collect())
        }

        async fn location_event_stream(
            &self,
            claimed_handle: &str,
            event_types: &[&str],
            since: DateTime<Utc>,
        ) -> Result<Vec<LatestLocationEvent>, RepoError> {
            // Same filter as location_trace but newest-LAST so the
            // dwell-walker can iterate forward in time.
            let mut rows: Vec<LatestLocationEvent> = self
                .rows
                .iter()
                .filter(|r| r.claimed_handle == claimed_handle)
                .filter_map(|r| {
                    let ts = r.event_timestamp?;
                    if ts < since || !event_types.contains(&r.event_type.as_str()) {
                        return None;
                    }
                    Some(LatestLocationEvent {
                        event_type: r.event_type.clone(),
                        event_timestamp: ts,
                        payload: r.payload.clone(),
                    })
                })
                .collect();
            rows.sort_by(|a, b| a.event_timestamp.cmp(&b.event_timestamp));
            Ok(rows)
        }

        async fn payload_field_breakdown(
            &self,
            claimed_handle: &str,
            event_type: &str,
            payload_field: &str,
            payload_filter: Option<PayloadFilter<'_>>,
            since: Option<DateTime<Utc>>,
            limit: i64,
        ) -> Result<Vec<PayloadFieldBucket>, RepoError> {
            use std::collections::HashMap;
            let mut counts: HashMap<String, i64> = HashMap::new();
            for r in &self.rows {
                if r.claimed_handle != claimed_handle || r.event_type != event_type {
                    continue;
                }
                if let Some(s) = since {
                    let Some(ts) = r.event_timestamp else {
                        continue;
                    };
                    if ts < s {
                        continue;
                    }
                }
                if let Some(filter) = payload_filter {
                    let actual = r.payload.get(filter.field).and_then(|v| v.as_str());
                    if actual != Some(filter.equals) {
                        continue;
                    }
                }
                if let Some(value) = r.payload.get(payload_field).and_then(|v| v.as_str()) {
                    *counts.entry(value.to_string()).or_insert(0) += 1;
                }
            }
            let mut rows: Vec<PayloadFieldBucket> = counts
                .into_iter()
                .map(|(value, count)| PayloadFieldBucket { value, count })
                .collect();
            rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
            rows.truncate(limit.max(0) as usize);
            Ok(rows)
        }

        async fn count_event_type(
            &self,
            claimed_handle: &str,
            event_type: &str,
            payload_filter: Option<PayloadFilter<'_>>,
            since: Option<DateTime<Utc>>,
        ) -> Result<u64, RepoError> {
            let mut n = 0u64;
            for r in &self.rows {
                if r.claimed_handle != claimed_handle || r.event_type != event_type {
                    continue;
                }
                if let Some(s) = since {
                    let Some(ts) = r.event_timestamp else {
                        continue;
                    };
                    if ts < s {
                        continue;
                    }
                }
                if let Some(filter) = payload_filter {
                    let actual = r.payload.get(filter.field).and_then(|v| v.as_str());
                    if actual != Some(filter.equals) {
                        continue;
                    }
                }
                n += 1;
            }
            Ok(n)
        }

        async fn latest_location(
            &self,
            claimed_handle: &str,
            event_types: &[&str],
        ) -> Result<LatestLocation, RepoError> {
            // Two passes over the same in-memory vec: one for the
            // most-recent location-bearing event, one for the most-
            // recent join_pu shard hint regardless of position.
            let mut location_event: Option<LatestLocationEvent> = None;
            let mut shard_hint: Option<(DateTime<Utc>, String)> = None;
            for r in &self.rows {
                if r.claimed_handle != claimed_handle {
                    continue;
                }
                let Some(ts) = r.event_timestamp else {
                    continue;
                };
                if event_types.contains(&r.event_type.as_str()) {
                    let better = match &location_event {
                        Some(le) => ts > le.event_timestamp,
                        None => true,
                    };
                    if better {
                        location_event = Some(LatestLocationEvent {
                            event_type: r.event_type.clone(),
                            event_timestamp: ts,
                            payload: r.payload.clone(),
                        });
                    }
                }
                if r.event_type == "join_pu" {
                    if let Some(s) = r.payload.get("shard").and_then(|v| v.as_str()) {
                        if shard_hint
                            .as_ref()
                            .map_or(true, |(prev_ts, _)| ts > *prev_ts)
                        {
                            shard_hint = Some((ts, s.to_string()));
                        }
                    }
                }
            }
            Ok(LatestLocation {
                location_event,
                shard_hint: shard_hint.map(|(_, s)| s),
            })
        }
    }
}

// -- Postgres store --------------------------------------------------

pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EventQuery for PostgresStore {
    async fn list_filtered(
        &self,
        claimed_handle: &str,
        filters: EventFilters,
    ) -> Result<Vec<StoredQueryEvent>, RepoError> {
        // Build the filter set with QueryBuilder so every value
        // hits the wire as a bound parameter — no string interpolation.
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "SELECT seq, claimed_handle, event_type, event_timestamp,
                    log_source, source_offset, payload
             FROM events
             WHERE claimed_handle = ",
        );
        qb.push_bind(claimed_handle);

        if let Some(t) = &filters.event_type {
            qb.push(" AND event_type = ");
            qb.push_bind(t.clone());
        }
        if let Some(s) = filters.since {
            qb.push(" AND event_timestamp >= ");
            qb.push_bind(s);
        }
        if let Some(u) = filters.until {
            qb.push(" AND event_timestamp <= ");
            qb.push_bind(u);
        }
        match filters.cursor {
            Some(SeqCursor::Before(n)) => {
                qb.push(" AND seq < ");
                qb.push_bind(n);
                qb.push(" ORDER BY seq DESC");
            }
            Some(SeqCursor::After(n)) => {
                qb.push(" AND seq > ");
                qb.push_bind(n);
                qb.push(" ORDER BY seq ASC");
            }
            None => {
                qb.push(" ORDER BY seq DESC");
            }
        }
        qb.push(" LIMIT ");
        qb.push_bind(filters.limit);

        let rows = qb
            .build_query_as::<(
                i64,
                String,
                String,
                Option<DateTime<Utc>>,
                String,
                i64,
                Value,
            )>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    seq,
                    claimed_handle,
                    event_type,
                    event_timestamp,
                    log_source,
                    source_offset,
                    payload,
                )| {
                    StoredQueryEvent {
                        seq,
                        claimed_handle,
                        event_type,
                        event_timestamp,
                        log_source,
                        source_offset,
                        payload,
                    }
                },
            )
            .collect())
    }

    async fn timeline(
        &self,
        claimed_handle: &str,
        days: u32,
    ) -> Result<Vec<(NaiveDate, i64)>, RepoError> {
        // `make_interval(days => $2)` keeps the day count as a real bound
        // parameter rather than being baked into the literal string.
        let rows: Vec<(NaiveDate, i64)> = sqlx::query_as(
            "SELECT (date_trunc('day', event_timestamp) AT TIME ZONE 'UTC')::date AS day,
                    COUNT(*)::BIGINT
             FROM events
             WHERE claimed_handle = $1
               AND event_timestamp IS NOT NULL
               AND event_timestamp >= NOW() - make_interval(days => $2)
             GROUP BY day
             ORDER BY day ASC",
        )
        .bind(claimed_handle)
        .bind(days as i32)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn summary_for_handle(
        &self,
        claimed_handle: &str,
    ) -> Result<(u64, Vec<(String, u64)>), RepoError> {
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE claimed_handle = $1")
                .bind(claimed_handle)
                .fetch_one(&self.pool)
                .await?;

        let by_type: Vec<(String, i64)> = sqlx::query_as(
            "SELECT event_type, COUNT(*)::BIGINT
             FROM events
             WHERE claimed_handle = $1
             GROUP BY event_type
             ORDER BY 2 DESC",
        )
        .bind(claimed_handle)
        .fetch_all(&self.pool)
        .await?;

        Ok((
            total.max(0) as u64,
            by_type
                .into_iter()
                .map(|(t, c)| (t, c.max(0) as u64))
                .collect(),
        ))
    }

    async fn event_type_breakdown(
        &self,
        claimed_handle: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<EventTypeStats>, RepoError> {
        // Two queries lets us keep the SQL simple. The first scopes
        // counts to the optional `since` window; the second lifts the
        // last_seen for each (unscoped) event_type so the column is
        // meaningful even if the type had zero rows in the window.
        let rows: Vec<(String, i64, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT event_type,
                    COUNT(*)::BIGINT AS count,
                    MAX(event_timestamp) AS last_seen
             FROM events
             WHERE claimed_handle = $1
               AND ($2::TIMESTAMPTZ IS NULL OR event_timestamp >= $2)
             GROUP BY event_type
             ORDER BY count DESC, event_type ASC",
        )
        .bind(claimed_handle)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(event_type, count, last_seen)| EventTypeStats {
                event_type,
                count,
                last_seen,
            })
            .collect())
    }

    async fn sessions_for_handle(
        &self,
        claimed_handle: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<InferredSession>, RepoError> {
        // Window-function pass:
        //  1. `gaps`: per row, look at the previous timestamp (LAG) and
        //     mark a session boundary when there's no previous row OR
        //     when the gap exceeds the idle threshold.
        //  2. `labeled`: cumulative SUM over those boundary flags
        //     becomes a session id (boundary=1 starts a new id).
        //  3. Outer GROUP BY collapses each session to start/end/count.
        //
        // The two-CTE form keeps the query plan readable; the inner
        // SELECT walks the events index by (claimed_handle, event_timestamp)
        // (already covered by `events_event_ts_idx` in 0001 + the existing
        // claimed_handle filter). LIMIT/OFFSET are applied to the
        // outer aggregate so pagination is over sessions, not rows.
        let gap_minutes = SESSION_IDLE_GAP_MINUTES;
        let rows: Vec<(DateTime<Utc>, DateTime<Utc>, i64)> = sqlx::query_as(
            "WITH gaps AS (
                SELECT event_timestamp,
                       LAG(event_timestamp) OVER (ORDER BY event_timestamp ASC) AS prev_ts
                FROM events
                WHERE claimed_handle = $1
                  AND event_timestamp IS NOT NULL
            ), labeled AS (
                SELECT event_timestamp,
                       SUM(CASE
                             WHEN prev_ts IS NULL
                               OR event_timestamp - prev_ts > make_interval(mins => $2)
                             THEN 1 ELSE 0
                           END) OVER (ORDER BY event_timestamp ASC) AS session_id
                FROM gaps
            )
            SELECT MIN(event_timestamp) AS start_at,
                   MAX(event_timestamp) AS end_at,
                   COUNT(*)::BIGINT     AS event_count
            FROM labeled
            GROUP BY session_id
            ORDER BY start_at DESC
            LIMIT $3 OFFSET $4",
        )
        .bind(claimed_handle)
        .bind(gap_minutes as i32)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(start_at, end_at, event_count)| InferredSession {
                start_at,
                end_at,
                event_count,
            })
            .collect())
    }

    async fn ingest_history_for_handle(
        &self,
        actor_handle: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IngestBatchRow>, RepoError> {
        // We pull each known field with a JSON path operator; `->>` on
        // a missing key yields NULL which we coerce to 0/None below.
        // Defensive — a future schema bump that drops fields shouldn't
        // crash the read endpoint.
        let rows: Vec<(
            i64,
            DateTime<Utc>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
        )> = sqlx::query_as(
            "SELECT seq,
                    occurred_at,
                    payload->>'batch_id'   AS batch_id,
                    payload->>'game_build' AS game_build,
                    NULLIF(payload->>'total','')::BIGINT     AS total,
                    NULLIF(payload->>'accepted','')::BIGINT  AS accepted,
                    NULLIF(payload->>'duplicate','')::BIGINT AS duplicate,
                    NULLIF(payload->>'rejected','')::BIGINT  AS rejected
             FROM audit_log
             WHERE action = 'ingest.batch_processed'
               AND actor_handle = $1
             ORDER BY seq DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(actor_handle)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(seq, occurred_at, batch_id, game_build, total, accepted, duplicate, rejected)| {
                    IngestBatchRow {
                        seq,
                        occurred_at,
                        batch_id: batch_id.unwrap_or_default(),
                        game_build,
                        total: total.unwrap_or(0),
                        accepted: accepted.unwrap_or(0),
                        duplicate: duplicate.unwrap_or(0),
                        rejected: rejected.unwrap_or(0),
                    }
                },
            )
            .collect())
    }

    async fn latest_location(
        &self,
        claimed_handle: &str,
        event_types: &[&str],
    ) -> Result<LatestLocation, RepoError> {
        // Two cheap reads. Both walk the existing
        // `events_event_ts_idx` index (claimed_handle + event_timestamp
        // DESC), filtered to the small set of event types we care about,
        // so the planner can stop after the first matching row.
        //
        // We pass the type list as a Postgres array — `event_type =
        // ANY($2)` is the canonical shape for "match against this small
        // set without bloating the query plan with an OR chain".
        let event_types_owned: Vec<String> = event_types.iter().map(|s| s.to_string()).collect();

        let location_row: Option<(String, DateTime<Utc>, Value)> = sqlx::query_as(
            "SELECT event_type, event_timestamp, payload
               FROM events
              WHERE claimed_handle = $1
                AND event_type = ANY($2)
                AND event_timestamp IS NOT NULL
              ORDER BY event_timestamp DESC
              LIMIT 1",
        )
        .bind(claimed_handle)
        .bind(&event_types_owned)
        .fetch_optional(&self.pool)
        .await?;

        let location_event = location_row.map(|(event_type, ts, payload)| LatestLocationEvent {
            event_type,
            event_timestamp: ts,
            payload,
        });

        // Independent fetch for the most recent shard hint. We don't
        // care about timestamp ordering relative to the location_event
        // — the resolver attaches the shard as contextual extra info
        // alongside whatever planet/city the location_event resolved.
        let shard_hint: Option<String> = sqlx::query_scalar(
            "SELECT payload->>'shard'
               FROM events
              WHERE claimed_handle = $1
                AND event_type = 'join_pu'
                AND payload ? 'shard'
              ORDER BY event_timestamp DESC NULLS LAST
              LIMIT 1",
        )
        .bind(claimed_handle)
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        Ok(LatestLocation {
            location_event,
            shard_hint,
        })
    }

    async fn location_event_stream(
        &self,
        claimed_handle: &str,
        event_types: &[&str],
        since: DateTime<Utc>,
    ) -> Result<Vec<LatestLocationEvent>, RepoError> {
        let event_types_owned: Vec<String> = event_types.iter().map(|s| s.to_string()).collect();
        let rows: Vec<(String, DateTime<Utc>, Value)> = sqlx::query_as(
            "SELECT event_type, event_timestamp, payload
               FROM events
              WHERE claimed_handle = $1
                AND event_type = ANY($2)
                AND event_timestamp IS NOT NULL
                AND event_timestamp >= $3
              ORDER BY event_timestamp ASC",
        )
        .bind(claimed_handle)
        .bind(&event_types_owned)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(event_type, event_timestamp, payload)| LatestLocationEvent {
                    event_type,
                    event_timestamp,
                    payload,
                },
            )
            .collect())
    }

    async fn payload_field_breakdown(
        &self,
        claimed_handle: &str,
        event_type: &str,
        payload_field: &str,
        payload_filter: Option<PayloadFilter<'_>>,
        since: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<Vec<PayloadFieldBucket>, RepoError> {
        // `payload->>$N` keeps the field name a parameter rather than
        // an interpolation, so even a hostile caller can't escape into
        // a different column. Filter is similarly parameterised — the
        // `$6 IS NULL OR ...` shape collapses to a no-op when the
        // caller passes None.
        let (filter_field, filter_value) = match payload_filter {
            Some(f) => (Some(f.field.to_string()), Some(f.equals.to_string())),
            None => (None, None),
        };
        let rows: Vec<(Option<String>, i64)> = sqlx::query_as(
            "SELECT payload->>$2 AS value, COUNT(*)::BIGINT AS count
               FROM events
              WHERE claimed_handle = $1
                AND event_type = $3
                AND ($4::timestamptz IS NULL OR event_timestamp >= $4)
                AND payload ? $2
                AND ($5::text IS NULL OR payload->>$5 = $6)
              GROUP BY payload->>$2
              ORDER BY count DESC, value ASC
              LIMIT $7",
        )
        .bind(claimed_handle)
        .bind(payload_field)
        .bind(event_type)
        .bind(since)
        .bind(filter_field)
        .bind(filter_value)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(value, count)| value.map(|v| PayloadFieldBucket { value: v, count }))
            .collect())
    }

    async fn count_event_type(
        &self,
        claimed_handle: &str,
        event_type: &str,
        payload_filter: Option<PayloadFilter<'_>>,
        since: Option<DateTime<Utc>>,
    ) -> Result<u64, RepoError> {
        let (filter_field, filter_value) = match payload_filter {
            Some(f) => (Some(f.field.to_string()), Some(f.equals.to_string())),
            None => (None, None),
        };
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT
               FROM events
              WHERE claimed_handle = $1
                AND event_type = $2
                AND ($3::timestamptz IS NULL OR event_timestamp >= $3)
                AND ($4::text IS NULL OR payload->>$4 = $5)",
        )
        .bind(claimed_handle)
        .bind(event_type)
        .bind(since)
        .bind(filter_field)
        .bind(filter_value)
        .fetch_one(&self.pool)
        .await?;
        Ok(count.max(0) as u64)
    }
}

#[async_trait]
impl EventStore for PostgresStore {
    async fn insert(&self, event: StoredEvent) -> Result<InsertOutcome, RepoError> {
        // ON CONFLICT lets Postgres tell us whether the row was new.
        // RETURNING (xmax = 0) is a Postgres-ism: xmax is 0 on a fresh
        // insert and non-zero when an existing row was updated; we
        // don't update on conflict so it stays 0 only for inserts.
        let inserted: bool = sqlx::query_scalar(
            r#"
            INSERT INTO events (
                id, idempotency_key, claimed_handle, event_type,
                event_timestamp, log_source, source_offset, raw_line, payload
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (claimed_handle, idempotency_key) DO NOTHING
            RETURNING TRUE
            "#,
        )
        .bind(event.id)
        .bind(&event.idempotency_key)
        .bind(&event.claimed_handle)
        .bind(&event.event_type)
        .bind(event.event_timestamp)
        .bind(log_source_to_str(event.log_source))
        .bind(event.source_offset)
        .bind(&event.raw_line)
        .bind(&event.payload)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false);

        Ok(if inserted {
            InsertOutcome::Inserted
        } else {
            InsertOutcome::Duplicate
        })
    }
}

fn log_source_to_str(s: LogSource) -> &'static str {
    match s {
        LogSource::Live => "live",
        LogSource::Ptu => "ptu",
        LogSource::Eptu => "eptu",
        LogSource::Hotfix => "hotfix",
        LogSource::Tech => "tech",
        LogSource::Other => "other",
    }
}

// -- In-memory store (test-only) -------------------------------------

#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    rows: Mutex<Vec<StoredEvent>>,
}

#[cfg(test)]
impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<StoredEvent> {
        self.rows.lock().expect("memory store poisoned").clone()
    }
}

#[cfg(test)]
#[async_trait]
impl EventStore for MemoryStore {
    async fn insert(&self, event: StoredEvent) -> Result<InsertOutcome, RepoError> {
        let mut rows = self.rows.lock().expect("memory store poisoned");
        let dup = rows.iter().any(|r| {
            r.claimed_handle == event.claimed_handle && r.idempotency_key == event.idempotency_key
        });
        if dup {
            return Ok(InsertOutcome::Duplicate);
        }
        rows.push(event);
        Ok(InsertOutcome::Inserted)
    }
}
