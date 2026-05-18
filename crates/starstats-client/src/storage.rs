//! Local SQLite event store. Wraps `rusqlite` behind a `Mutex` so the
//! tail loop and Tauri command handlers can share it.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use starstats_core::unknown_lines::UnknownLine;
use std::path::Path;
use std::sync::Mutex;

/// Surfacing threshold matching the spec — lines below this never make
/// it into the review queue by default. Callers can pass a lower (or
/// zero) cutoff if they want to inspect everything that was captured.
/// Exposed so the eventual Tauri command + UI badge agree on the cutoff
/// without each importing the literal `50`.
#[allow(dead_code)]
pub const UNKNOWN_LINE_MIN_INTEREST: u8 = 50;

/// Cap on `raw_examples_json` entries. Keep this tight: reviewers only
/// need a handful of concrete samples to sanity-check a shape, and the
/// JSON blob is read whole on every upsert.
#[allow(dead_code)]
const RAW_EXAMPLES_CAP: usize = 5;

const SCHEMA: &str = include_str!("../sql/schema.sql");

pub struct Storage {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct UnsentEvent {
    pub id: i64,
    pub idempotency_key: String,
    pub payload_json: String,
    pub raw_line: String,
    pub log_source: String,
    pub source_offset: u64,
}

/// One row from `events`, restricted to the columns the timeline UI
/// needs. `payload_json` is the same string we wrote on insert, so
/// callers can deserialise it back into a `GameEvent` for formatting.
/// `raw_line` is the exact log line as captured from disk; the Logs
/// pane surfaces it in the per-event detail drawer for forensic
/// inspection. `log_source` is the channel tag (LIVE/PTU/EPTU) so the
/// drawer's Source row reflects which build the event came from rather
/// than guessing.
#[derive(Debug, Clone)]
pub struct RecentEventRow {
    pub id: i64,
    pub event_type: String,
    pub timestamp: String,
    pub payload_json: String,
    pub raw_line: String,
    pub log_source: String,
}

/// Full row from `events` — used by the re-parse iterator. Has every
/// column the classifier could need to either re-score the line or
/// rewrite the payload in place. Several fields are not consumed by
/// the current re-parse path but are kept on the struct so future
/// passes (e.g. backfill-with-rules-applied) don't need to widen the
/// SELECT shape.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EventRow {
    pub id: i64,
    pub idempotency_key: String,
    pub event_type: String,
    pub timestamp: String,
    pub raw_line: String,
    pub payload_json: String,
    pub log_source: String,
    pub source_offset: u64,
}

/// Lean projection used by the retro-burst phase of re-parse. Carries
/// only the columns `detect_bursts` needs (raw line for
/// `structural_parse`, offset for the idempotency key) plus the row
/// `id` so members can be deleted after the summary is inserted, and
/// `event_type` so already-collapsed `burst_summary` rows can be
/// trivially skipped without re-parsing them.
#[derive(Debug, Clone)]
pub struct BurstScanRow {
    pub id: i64,
    pub raw_line: String,
    pub source_offset: u64,
    pub event_type: String,
}

/// One row from `unknown_event_samples`. Mirrors every non-id column
/// in the table; the command layer maps it onto its serialised wire
/// counterpart.
#[derive(Debug, Clone)]
pub struct UnknownSampleRow {
    pub log_source: String,
    pub event_name: String,
    pub occurrences: u64,
    pub first_seen: String,
    pub last_seen: String,
    pub sample_line: String,
    pub sample_body: String,
}

/// Engine-internal events we know aren't worth surfacing as
/// candidate parser rules. Seeded into `event_noise_list` on first
/// run; users can extend this from the UI later. Keep this list
/// conservative — anything ambiguous (could plausibly carry player
/// signal) belongs in the unknowns table until classified.
const DEFAULT_NOISE: &[&str] = &[
    // Asset cache chatter.
    "StatObjLoad 0x800 Format",
    // Engine state machine — fires hundreds of times per session.
    "CContextEstablisherStepStart",
    "ContextEstablisherTaskFinished",
    "Context Establisher Blocked",
    "Context Establisher Unblocked",
    "Context Establisher Done",
    "ContextEstablisher Model Change State",
    "ContextEstablisher State Change Delivery Result",
    "ContextEstablisher Send State Change",
    "ContextEstablisher Remote Change State Success",
    // Hangar elevator / loading-platform internals.
    "CSCLoadingPlatformManager::TransitionLightGroupState",
    "CSCLoadingPlatformManager::LoadEntitiesReference",
    "CSCLoadingPlatformManager::LoadEntitiesReference::<lambda_1>::operator ()",
    "CSCLoadingPlatformManager::OnLoadingPlatformStateChanged",
    "CSCLoadingPlatformManager::StopEffectForAllTags",
    "CSCLoadingPlatformManager::loadEntityFromXML::<lambda_1>::operator ()",
    "LoadingPlatformUtilities::LoadFromXmlNode",
    // Misc engine bookkeeping.
    "Update group cache",
    "SerializedOverwrite",
    "RegisterUniverseHierarchy_End",
    "ReuseChannel",
    "Stream started",
    "[BuildingBlocks] Invalid Url",
    "ProximitySensorMakingLocalHelper",
];

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("create db dir")?;
        }
        let conn = Connection::open(path).context("open sqlite")?;
        conn.execute_batch(SCHEMA).context("apply schema")?;
        Self::seed_default_noise(&conn).context("seed default noise list")?;
        Self::purge_noise_from_unknowns(&conn).context("purge stale noise samples")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn seed_default_noise(conn: &Connection) -> Result<()> {
        // Drop obsolete shipped builtins whose captured form changed
        // when the parser learned to handle nested `<...>` symbols.
        // Only touches rows tagged `builtin` — user-added entries with
        // the same name are kept (they'd have source='user').
        conn.execute(
            "DELETE FROM event_noise_list
             WHERE source = 'builtin'
               AND event_name LIKE '%::<lambda%'
               AND event_name NOT LIKE '%operator ()%'",
            [],
        )?;

        // Idempotently insert every shipped builtin. ON CONFLICT keeps
        // user-added entries with the same name from being clobbered.
        for name in DEFAULT_NOISE {
            conn.execute(
                "INSERT INTO event_noise_list(event_name, source) VALUES (?, 'builtin')
                 ON CONFLICT(event_name) DO NOTHING",
                params![name],
            )?;
        }
        Ok(())
    }

    /// One-shot cleanup: when the app boots and the noise list shifts
    /// (new defaults shipped, user added an entry), retroactively drop
    /// matching rows from `unknown_event_samples` so the actionable
    /// list stays clean. Cheap — the unknowns table is small.
    ///
    /// Also drops any malformed legacy rows whose `event_name` was
    /// captured by an older parser that truncated nested `<...>`
    /// symbols. A correctly-captured event with embedded `<lambda_N>`
    /// always ends in `>`; anything matching `*::<lambda*` that does
    /// NOT end in `>` was produced by the buggy parser.
    fn purge_noise_from_unknowns(conn: &Connection) -> Result<()> {
        conn.execute(
            "DELETE FROM unknown_event_samples
             WHERE event_name IN (SELECT event_name FROM event_noise_list)",
            [],
        )?;
        conn.execute(
            "DELETE FROM unknown_event_samples
             WHERE event_name LIKE '%::<lambda%'
               AND event_name NOT LIKE '%>'",
            [],
        )?;
        Ok(())
    }

    /// Cheap membership test used by the tailer's hot path.
    pub fn is_noise(&self, event_name: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM event_noise_list WHERE event_name = ?",
            params![event_name],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    /// Add an event_name to the noise list. `source` is informational
    /// — typically `"user"` from the tray UI, `"builtin"` from the
    /// seeded defaults, or `"community"` from a future rule-sync feed.
    pub fn add_noise(&self, event_name: &str, source: &str) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            "INSERT INTO event_noise_list(event_name, source) VALUES (?, ?)
             ON CONFLICT(event_name) DO NOTHING",
            params![event_name, source],
        )?;
        // Also drop any existing samples — they're noise now.
        conn.execute(
            "DELETE FROM unknown_event_samples WHERE event_name = ?",
            params![event_name],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_event(
        &self,
        idempotency_key: &str,
        event_type: &str,
        timestamp: &str,
        raw: &str,
        payload_json: &str,
        log_source: &str,
        source_offset: u64,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        // ON CONFLICT keeps the table append-only-ish: same line
        // re-tailed (after a rotation/replay) won't double-insert.
        conn.execute(
            "INSERT INTO events
                (idempotency_key, type, timestamp, raw, payload, log_source, source_offset)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(idempotency_key) DO NOTHING",
            params![
                idempotency_key,
                event_type,
                timestamp,
                raw,
                payload_json,
                log_source,
                source_offset as i64,
            ],
        )?;
        Ok(())
    }

    /// Read up to `limit` events with `id > after_id`, ordered by id.
    /// Returns (id, idempotency_key, payload_json, raw, log_source, source_offset).
    pub fn read_unsent(&self, after_id: i64, limit: usize) -> Result<Vec<UnsentEvent>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, idempotency_key, payload, raw, log_source, source_offset
             FROM events
             WHERE id > ?
             ORDER BY id ASC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![after_id, limit as i64], |row| {
            Ok(UnsentEvent {
                id: row.get(0)?,
                idempotency_key: row.get(1)?,
                payload_json: row.get(2)?,
                raw_line: row.get(3)?,
                log_source: row.get(4)?,
                source_offset: row.get::<_, i64>(5)?.max(0) as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn read_sync_cursor(&self) -> Result<i64> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let result: rusqlite::Result<i64> =
            conn.query_row("SELECT last_event_id FROM sync_cursor", [], |row| {
                row.get(0)
            });
        match result {
            Ok(n) => Ok(n.max(0)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_sync_cursor(&self, last_event_id: i64) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        // sync_cursor is a singleton row — update or insert.
        let n = conn.execute(
            "UPDATE sync_cursor SET last_event_id = ?, updated_at = datetime('now')",
            params![last_event_id],
        )?;
        if n == 0 {
            conn.execute(
                "INSERT INTO sync_cursor(last_event_id) VALUES (?)",
                params![last_event_id],
            )?;
        }
        Ok(())
    }

    pub fn read_cursor(&self, source_path: &str) -> Result<u64> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let result: rusqlite::Result<i64> = conn.query_row(
            "SELECT offset FROM tail_cursor WHERE path = ?",
            params![source_path],
            |row| row.get(0),
        );
        match result {
            Ok(n) => Ok(n.max(0) as u64),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_cursor(&self, source_path: &str, offset: u64) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            "INSERT INTO tail_cursor(path, offset) VALUES (?, ?)
             ON CONFLICT(path) DO UPDATE SET
                 offset = excluded.offset,
                 updated_at = datetime('now')",
            params![source_path, offset as i64],
        )?;
        Ok(())
    }

    pub fn event_counts(&self) -> Result<Vec<(String, u64)>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn
            .prepare("SELECT type, COUNT(*) FROM events GROUP BY type ORDER BY 2 DESC LIMIT 50")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn total_events(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// UPSERT one observation of an unknown event. First call inserts
    /// with `occurrences = 1` (table default). Subsequent calls bump
    /// `occurrences` and refresh the sample so the most recent body
    /// is always available for inspection.
    pub fn record_unknown(
        &self,
        log_source: &str,
        event_name: &str,
        sample_line: &str,
        sample_body: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            "INSERT INTO unknown_event_samples (log_source, event_name, sample_line, sample_body)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(log_source, event_name) DO UPDATE SET
                 occurrences = occurrences + 1,
                 last_seen = datetime('now'),
                 sample_line = excluded.sample_line,
                 sample_body = excluded.sample_body",
            params![log_source, event_name, sample_line, sample_body],
        )?;
        Ok(())
    }

    /// Most recent events, newest first. Used to render a chronological
    /// "what happened" timeline in the tray UI.
    pub fn recent_events(&self, limit: usize) -> Result<Vec<RecentEventRow>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, type, timestamp, payload, raw, log_source
             FROM events
             ORDER BY timestamp DESC, id DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(RecentEventRow {
                id: row.get(0)?,
                event_type: row.get(1)?,
                timestamp: row.get(2)?,
                payload_json: row.get(3)?,
                raw_line: row.get(4)?,
                log_source: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Total bytes the SQLite file occupies on disk, computed via
    /// `page_count * page_size`. Cheap (single round-trip per pragma)
    /// and avoids a filesystem stat that may disagree with the
    /// engine's view if WAL pages are still in flight.
    pub fn database_size_bytes(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let bytes = page_count.max(0) as u64 * page_size.max(0) as u64;
        Ok(bytes)
    }

    /// Return the top `limit` unknown events, most-seen first, ties
    /// broken by most-recently-seen.
    pub fn recent_unknowns(&self, limit: usize) -> Result<Vec<UnknownSampleRow>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT log_source, event_name, occurrences, first_seen, last_seen,
                    sample_line, sample_body
             FROM unknown_event_samples
             ORDER BY occurrences DESC, last_seen DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(UnknownSampleRow {
                log_source: row.get(0)?,
                event_name: row.get(1)?,
                occurrences: row.get::<_, i64>(2)?.max(0) as u64,
                first_seen: row.get(3)?,
                last_seen: row.get(4)?,
                sample_line: row.get(5)?,
                sample_body: row.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Persist the active parser-definition manifest. The cache holds
    /// at most one row (sentinel `id = 1`); an UPSERT replaces it on
    /// every successful fetch. We store the raw JSON so a future
    /// schema_version bump can be handled by re-deserialising in the
    /// new shape without a migration.
    pub fn write_parser_def_manifest(&self, version: u32, payload_json: &str) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            "INSERT INTO parser_def_manifest (id, version, payload_json)
             VALUES (1, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               version = excluded.version,
               fetched_at = datetime('now'),
               payload_json = excluded.payload_json",
            params![version as i64, payload_json],
        )?;
        Ok(())
    }

    /// Stream every row of `events` for re-parse. Loads the full set
    /// in batches so a multi-million-row store doesn't materialize as
    /// one giant `Vec`. Caller closure decides what to do per row;
    /// returning `Err` aborts the iteration.
    pub fn for_each_event<F>(&self, batch_size: usize, mut f: F) -> Result<()>
    where
        F: FnMut(EventRow) -> Result<()>,
    {
        let mut last_id: i64 = 0;
        loop {
            // Fetch one batch with the lock held, then release it
            // BEFORE invoking the closure. The original implementation
            // held the lock across the closure body, which deadlocked
            // any caller (e.g. reparse) that called back into other
            // Storage methods that re-acquire the same lock. Paged by
            // `id > last_id` so concurrent inserts during the walk
            // are visited in a later batch — correct for re-parse.
            let conn = self.conn.lock().expect("storage mutex poisoned");
            let mut stmt = conn.prepare(
                "SELECT id, idempotency_key, type, timestamp, raw, payload, log_source, source_offset
                 FROM events
                 WHERE id > ?
                 ORDER BY id ASC
                 LIMIT ?",
            )?;
            let mapped = stmt.query_map(params![last_id, batch_size as i64], |row| {
                Ok(EventRow {
                    id: row.get(0)?,
                    idempotency_key: row.get(1)?,
                    event_type: row.get(2)?,
                    timestamp: row.get(3)?,
                    raw_line: row.get(4)?,
                    payload_json: row.get(5)?,
                    log_source: row.get(6)?,
                    source_offset: row.get::<_, i64>(7)?.max(0) as u64,
                })
            })?;
            let rows: Vec<EventRow> = mapped.filter_map(|r| r.ok()).collect();
            // Drop the lock guard explicitly so the closure invoked
            // below is free to call back into other Storage methods
            // that re-acquire the connection. Without this drop the
            // re-parse closure deadlocks the moment it tries to write
            // an updated classification.
            drop(stmt);
            drop(conn);
            if rows.is_empty() {
                break;
            }
            for row in rows {
                last_id = row.id;
                f(row)?;
            }
        }
        Ok(())
    }

    /// Re-classify in place — overwrite an existing row's type +
    /// payload + timestamp (timestamps can refine when a richer
    /// classifier extracts a more precise field). Used by the
    /// re-parse command when newer rules upgrade an existing match.
    /// Returns the number of rows actually updated (0 or 1).
    pub fn update_event_classification(
        &self,
        id: i64,
        event_type: &str,
        timestamp: &str,
        payload_json: &str,
    ) -> Result<usize> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let n = conn.execute(
            "UPDATE events SET type = ?, timestamp = ?, payload = ? WHERE id = ?",
            params![event_type, timestamp, payload_json, id],
        )?;
        Ok(n)
    }

    /// Distinct `log_source` values present in `events`. Used by the
    /// retro-burst phase of re-parse to walk one source's history at a
    /// time so detect_bursts sees a single contiguous source-offset
    /// stream rather than an interleaved multi-channel mix.
    pub fn distinct_log_sources(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT DISTINCT log_source FROM events ORDER BY log_source")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Lean projection of one `log_source`'s events, ordered by
    /// `source_offset` (then `id` as a stable tiebreaker). Skips the
    /// payload column because retro-burst only needs the raw line for
    /// `structural_parse` and the offset for the idempotency key.
    pub fn events_for_burst_scan(&self, log_source: &str) -> Result<Vec<BurstScanRow>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, raw, source_offset, type
             FROM events
             WHERE log_source = ?
             ORDER BY source_offset ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![log_source], |row| {
            Ok(BurstScanRow {
                id: row.get(0)?,
                raw_line: row.get(1)?,
                source_offset: row.get::<_, i64>(2)?.max(0) as u64,
                event_type: row.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Hard-delete a single event row by id. Used by retro-burst to
    /// suppress members that have been collapsed into a synthesised
    /// `BurstSummary`. We delete rather than soft-delete because the
    /// timeline reader has no notion of a tombstone column and a
    /// soft-delete would force every read site to learn one.
    pub fn delete_event_by_id(&self, id: i64) -> Result<usize> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let n = conn.execute("DELETE FROM events WHERE id = ?", params![id])?;
        Ok(n)
    }

    /// Drop a single unknown sample by `(log_source, event_name)`.
    /// Used by re-parse: once a sample line has been promoted to a
    /// real `events` row, the unknown record is no longer the
    /// "actionable next thing to write a rule for".
    pub fn delete_unknown(&self, log_source: &str, event_name: &str) -> Result<usize> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let n = conn.execute(
            "DELETE FROM unknown_event_samples
             WHERE log_source = ? AND event_name = ?",
            params![log_source, event_name],
        )?;
        Ok(n)
    }

    /// Read the cached manifest payload, if any. Returns `Ok(None)`
    /// for first-run when the cache is empty (the caller should treat
    /// this as "no remote rules" and continue).
    pub fn read_parser_def_manifest(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare("SELECT payload_json FROM parser_def_manifest WHERE id = 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            Ok(Some(payload))
        } else {
            Ok(None)
        }
    }

    /// Upsert one captured `UnknownLine` keyed by `shape_hash`. New
    /// rows insert verbatim; existing rows bump `occurrence_count`,
    /// refresh `last_seen`, and append `line.raw_line` to the cached
    /// raw-examples buffer — dropping the oldest entry when the cap is
    /// exceeded so the buffer stays bounded. Other fields on a
    /// duplicate (interest score, partial_structured, context) are
    /// left untouched; the first capture sets the canonical record.
    #[allow(dead_code)]
    pub fn cache_unknown_line(&self, line: &UnknownLine) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let existing: rusqlite::Result<(i64, String)> = conn.query_row(
            "SELECT occurrence_count, raw_examples_json
             FROM unknown_lines
             WHERE shape_hash = ?",
            params![line.shape_hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match existing {
            Ok((count, raw_json)) => {
                let mut samples: Vec<String> = serde_json::from_str(&raw_json)
                    .context("decode raw_examples_json on upsert")?;
                samples.push(line.raw_line.clone());
                while samples.len() > RAW_EXAMPLES_CAP {
                    samples.remove(0);
                }
                let new_raw = serde_json::to_string(&samples)
                    .context("encode raw_examples_json on upsert")?;
                conn.execute(
                    "UPDATE unknown_lines SET
                        occurrence_count = ?,
                        last_seen = ?,
                        raw_examples_json = ?
                     WHERE shape_hash = ?",
                    params![count + 1, line.last_seen, new_raw, line.shape_hash],
                )?;
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let raw_examples = serde_json::to_string(&vec![line.raw_line.clone()])
                    .context("encode raw_examples_json on insert")?;
                let partial = serde_json::to_string(&line.partial_structured)
                    .context("encode partial_structured_json")?;
                let context_before = serde_json::to_string(&line.context_before)
                    .context("encode context_before_json")?;
                let context_after = serde_json::to_string(&line.context_after)
                    .context("encode context_after_json")?;
                let pii = serde_json::to_string(&line.detected_pii)
                    .context("encode detected_pii_json")?;
                let channel = serde_json::to_value(line.channel)
                    .context("encode channel")?
                    .as_str()
                    .map(str::to_string)
                    .context("channel serialises to a string")?;
                conn.execute(
                    "INSERT INTO unknown_lines (
                        id, shape_hash, raw_examples_json, partial_structured_json,
                        shell_tag, context_before_json, context_after_json,
                        game_build, channel, interest_score, occurrence_count,
                        first_seen, last_seen, detected_pii_json, dismissed, submitted_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)",
                    params![
                        line.id,
                        line.shape_hash,
                        raw_examples,
                        partial,
                        line.shell_tag,
                        context_before,
                        context_after,
                        line.game_build,
                        channel,
                        line.interest_score as i64,
                        line.occurrence_count as i64,
                        line.first_seen,
                        line.last_seen,
                        pii,
                        if line.dismissed { 1_i64 } else { 0_i64 },
                    ],
                )?;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// Surface every non-dismissed unknown line whose interest score
    /// meets `min_interest`. Ordered by interest desc, occurrence_count
    /// desc, last_seen desc so the most actionable shapes float to the
    /// top of the review pane. `min_interest = 50` matches the spec
    /// default; callers can lower the bar for diagnostic views.
    pub fn list_unknown_lines(&self, min_interest: u8) -> Result<Vec<UnknownLine>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, shape_hash, raw_examples_json, partial_structured_json,
                    shell_tag, context_before_json, context_after_json,
                    game_build, channel, interest_score, occurrence_count,
                    first_seen, last_seen, detected_pii_json, dismissed, submitted_at
             FROM unknown_lines
             WHERE dismissed = 0 AND interest_score >= ?
             ORDER BY interest_score DESC, occurrence_count DESC, last_seen DESC",
        )?;
        let rows = stmt.query_map(params![min_interest as i64], decode_unknown_line)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Count non-dismissed unknown lines at or above the given
    /// interest cutoff. Tray badge calls this on a timer so it stays
    /// cheap — the dedicated index on `(dismissed, interest_score)`
    /// keeps the scan small.
    pub fn count_unknown_lines(&self, min_interest: u8) -> Result<u32> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM unknown_lines
             WHERE dismissed = 0 AND interest_score >= ?",
            params![min_interest as i64],
            |row| row.get(0),
        )?;
        Ok(n.max(0) as u32)
    }

    /// Mark a shape as dismissed so it never resurfaces in
    /// `list_unknown_lines`. The row is kept (not deleted) so a future
    /// re-capture of the same shape doesn't re-trigger the badge.
    pub fn dismiss_unknown_line(&self, shape_hash: &str) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            "UPDATE unknown_lines SET dismissed = 1 WHERE shape_hash = ?",
            params![shape_hash],
        )?;
        Ok(())
    }

    /// Stamp a shape with its submission timestamp once the row has
    /// been shipped to the server's moderation queue. The caller owns
    /// the timestamp format (ISO-8601 by convention) so this method
    /// stays a thin write.
    pub fn mark_submitted(&self, shape_hash: &str, submitted_at: &str) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            "UPDATE unknown_lines SET submitted_at = ? WHERE shape_hash = ?",
            params![submitted_at, shape_hash],
        )?;
        Ok(())
    }

    /// Test/debug helper — return just the cached raw-line samples for
    /// a given shape. Used by the `raw_examples_cap_at_five` test to
    /// inspect the buffer without depending on `UnknownLine.raw_line`.
    #[cfg(test)]
    pub fn list_raw_examples(&self, shape_hash: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let raw: String = conn.query_row(
            "SELECT raw_examples_json FROM unknown_lines WHERE shape_hash = ?",
            params![shape_hash],
            |row| row.get(0),
        )?;
        let samples: Vec<String> =
            serde_json::from_str(&raw).context("decode raw_examples_json")?;
        Ok(samples)
    }
}

/// Decode one `unknown_lines` row back into an `UnknownLine`. Kept as
/// a free function so both `list_unknown_lines` and any future
/// single-row reader can share the column order.
#[allow(dead_code)]
fn decode_unknown_line(row: &rusqlite::Row<'_>) -> rusqlite::Result<UnknownLine> {
    let partial_json: String = row.get(3)?;
    let context_before_json: String = row.get(5)?;
    let context_after_json: String = row.get(6)?;
    let detected_pii_json: String = row.get(13)?;
    let channel_str: String = row.get(8)?;
    let dismissed_i: i64 = row.get(14)?;

    let partial = serde_json::from_str(&partial_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let context_before = serde_json::from_str(&context_before_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let context_after = serde_json::from_str(&context_after_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let detected_pii = serde_json::from_str(&detected_pii_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let channel = serde_json::from_value(serde_json::Value::String(channel_str)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let interest_i: i64 = row.get(9)?;
    let occurrence_i: i64 = row.get(10)?;

    Ok(UnknownLine {
        id: row.get(0)?,
        shape_hash: row.get(1)?,
        // The `raw_line` field on the returned UnknownLine reflects
        // the MOST RECENT sample we've stashed for this shape — the
        // canonical buffer is `raw_examples_json` on disk, but
        // callers that only want one example expect the freshest.
        raw_line: {
            let raw_examples_json: String = row.get(2)?;
            let samples: Vec<String> = serde_json::from_str(&raw_examples_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            samples.last().cloned().unwrap_or_default()
        },
        timestamp: None,
        shell_tag: row.get(4)?,
        partial_structured: partial,
        context_before,
        context_after,
        game_build: row.get(7)?,
        channel,
        interest_score: interest_i.clamp(0, u8::MAX as i64) as u8,
        occurrence_count: occurrence_i.max(0) as u32,
        first_seen: row.get(11)?,
        last_seen: row.get(12)?,
        detected_pii,
        dismissed: dismissed_i != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_storage() -> (Storage, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("test.sqlite3");
        let storage = Storage::open(&path).expect("open storage");
        (storage, dir)
    }

    #[test]
    fn record_unknown_dedupes_by_source_and_event() {
        let (storage, _tmp) = fresh_storage();

        storage
            .record_unknown("live", "Foo", "raw line v1", "body v1")
            .expect("first record");
        storage
            .record_unknown("live", "Foo", "raw line v2", "body v2")
            .expect("second record");

        let rows = storage.recent_unknowns(50).expect("read unknowns");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.log_source, "live");
        assert_eq!(row.event_name, "Foo");
        assert_eq!(row.occurrences, 2);
        // Sample is overwritten with the latest call's value.
        assert_eq!(row.sample_body, "body v2");
        assert_eq!(row.sample_line, "raw line v2");
    }

    #[test]
    fn recent_unknowns_orders_by_occurrences_desc() {
        let (storage, _tmp) = fresh_storage();

        // Bar: 1 occurrence
        storage
            .record_unknown("live", "Bar", "rawB", "bodyB")
            .expect("Bar");
        // Foo: 3 occurrences
        for _ in 0..3 {
            storage
                .record_unknown("live", "Foo", "rawF", "bodyF")
                .expect("Foo");
        }

        let rows = storage.recent_unknowns(50).expect("read unknowns");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].event_name, "Foo");
        assert_eq!(rows[0].occurrences, 3);
        assert_eq!(rows[1].event_name, "Bar");
        assert_eq!(rows[1].occurrences, 1);
    }

    #[test]
    fn fresh_db_seeds_default_noise() {
        let (storage, _tmp) = fresh_storage();
        // Pick any well-known default — StatObjLoad is the heaviest noise source.
        assert!(storage.is_noise("StatObjLoad 0x800 Format").unwrap());
        // Random unknown name is not noise.
        assert!(!storage.is_noise("Some Player Event").unwrap());
    }

    #[test]
    fn add_noise_dedupes_and_drops_existing_samples() {
        let (storage, _tmp) = fresh_storage();
        // Pre-record an unknown that we're about to mark as noise.
        storage
            .record_unknown("live", "ChattyEvent", "raw", "body")
            .unwrap();
        assert_eq!(storage.recent_unknowns(50).unwrap().len(), 1);

        storage.add_noise("ChattyEvent", "user").unwrap();
        // Sample purged.
        assert_eq!(storage.recent_unknowns(50).unwrap().len(), 0);
        // Membership reflected.
        assert!(storage.is_noise("ChattyEvent").unwrap());

        // Idempotent — second add is a no-op.
        storage.add_noise("ChattyEvent", "user").unwrap();
        assert!(storage.is_noise("ChattyEvent").unwrap());
    }

    #[test]
    fn purge_runs_at_open_time() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.sqlite3");

        // Stage 1: open, record unknowns, including one we'll later
        // promote to noise via a hand-written insert.
        {
            let s = Storage::open(&path).unwrap();
            s.record_unknown("live", "ToBecomeNoise", "raw", "body")
                .unwrap();
            s.record_unknown("live", "StaysAsUnknown", "raw", "body")
                .unwrap();
            // Manually mark one as noise without calling add_noise()
            // — simulates a noise list shipped via app update.
            s.conn
                .lock()
                .unwrap()
                .execute(
                    "INSERT INTO event_noise_list(event_name, source) VALUES (?, 'builtin')",
                    params!["ToBecomeNoise"],
                )
                .unwrap();
        }

        // Stage 2: re-open. Purge should drop the now-noise sample.
        let s = Storage::open(&path).unwrap();
        let rows = s.recent_unknowns(50).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_name, "StaysAsUnknown");
    }

    #[test]
    fn record_unknown_separates_log_sources() {
        let (storage, _tmp) = fresh_storage();

        storage
            .record_unknown("live", "Foo", "raw", "body")
            .expect("live");
        storage
            .record_unknown("ptu", "Foo", "raw", "body")
            .expect("ptu");

        let rows = storage.recent_unknowns(50).expect("read unknowns");
        assert_eq!(rows.len(), 2);
        let mut sources: Vec<&str> = rows.iter().map(|r| r.log_source.as_str()).collect();
        sources.sort();
        assert_eq!(sources, vec!["live", "ptu"]);
        for row in &rows {
            assert_eq!(row.event_name, "Foo");
            assert_eq!(row.occurrences, 1);
        }
    }

    // ─── Phase 4.B unknown_lines cache ─────────────────────────────

    use starstats_core::unknown_lines::UnknownLine;
    use starstats_core::wire::LogSource;
    use std::collections::BTreeMap;

    fn make_unknown_line(shape_hash: &str, interest_score: u8) -> UnknownLine {
        UnknownLine {
            id: format!("id-{shape_hash}"),
            raw_line: format!("raw for {shape_hash}"),
            timestamp: None,
            shell_tag: Some("ShellTag".to_string()),
            partial_structured: BTreeMap::new(),
            context_before: Vec::new(),
            context_after: Vec::new(),
            game_build: None,
            channel: LogSource::Live,
            interest_score,
            shape_hash: shape_hash.to_string(),
            occurrence_count: 1,
            first_seen: "2026-05-17T14:02:30Z".to_string(),
            last_seen: "2026-05-17T14:02:30Z".to_string(),
            detected_pii: Vec::new(),
            dismissed: false,
        }
    }

    #[test]
    fn upsert_increments_count_on_same_shape() {
        let (storage, _tmp) = fresh_storage();
        let line = make_unknown_line("shape_a", 60);
        storage.cache_unknown_line(&line).unwrap();
        storage.cache_unknown_line(&line).unwrap();
        let rows = storage.list_unknown_lines(50).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].occurrence_count, 2);
    }

    #[test]
    fn raw_examples_cap_at_five() {
        let (storage, _tmp) = fresh_storage();
        let mut line = make_unknown_line("shape_a", 60);
        for i in 0..7 {
            line.raw_line = format!("line {i}");
            storage.cache_unknown_line(&line).unwrap();
        }
        let raws = storage.list_raw_examples("shape_a").unwrap();
        assert!(raws.len() <= 5);
        assert!(raws.last().unwrap().contains("line 6"));
    }

    #[test]
    fn list_filters_by_threshold_and_dismissed() {
        let (storage, _tmp) = fresh_storage();
        storage
            .cache_unknown_line(&make_unknown_line("a", 80))
            .unwrap();
        storage
            .cache_unknown_line(&make_unknown_line("b", 30))
            .unwrap();
        storage
            .cache_unknown_line(&make_unknown_line("c", 90))
            .unwrap();
        storage.dismiss_unknown_line("c").unwrap();
        let rows = storage.list_unknown_lines(50).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].shape_hash, "a");
    }

    #[test]
    fn mark_submitted_sets_timestamp() {
        let (storage, _tmp) = fresh_storage();
        storage
            .cache_unknown_line(&make_unknown_line("shape_x", 70))
            .unwrap();
        storage
            .mark_submitted("shape_x", "2026-05-17T15:00:00Z")
            .unwrap();

        // Read back the raw submitted_at column to verify it landed —
        // the public list path doesn't surface submitted_at on
        // UnknownLine because the wire type predates persistence.
        let conn = storage.conn.lock().unwrap();
        let submitted: Option<String> = conn
            .query_row(
                "SELECT submitted_at FROM unknown_lines WHERE shape_hash = ?",
                params!["shape_x"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(submitted.as_deref(), Some("2026-05-17T15:00:00Z"));
    }

    #[test]
    fn count_unknown_lines_returns_dismissed_filtered_count() {
        let (storage, _tmp) = fresh_storage();
        storage
            .cache_unknown_line(&make_unknown_line("a", 80))
            .unwrap();
        storage
            .cache_unknown_line(&make_unknown_line("b", 60))
            .unwrap();
        storage
            .cache_unknown_line(&make_unknown_line("c", 30))
            .unwrap();
        storage
            .cache_unknown_line(&make_unknown_line("d", 90))
            .unwrap();
        storage.dismiss_unknown_line("d").unwrap();

        // 2 rows above threshold AND not dismissed: a (80) and b (60).
        // c is below threshold; d is dismissed.
        assert_eq!(storage.count_unknown_lines(50).unwrap(), 2);
        // Drop the threshold — c counts but d still doesn't.
        assert_eq!(storage.count_unknown_lines(0).unwrap(), 3);
    }

    #[test]
    fn list_orders_by_interest_then_occurrence() {
        let (storage, _tmp) = fresh_storage();
        // Upsert "a" once with score 60.
        storage
            .cache_unknown_line(&make_unknown_line("a", 60))
            .unwrap();
        // Upsert "b" twice with score 60 — same interest, higher count.
        storage
            .cache_unknown_line(&make_unknown_line("b", 60))
            .unwrap();
        storage
            .cache_unknown_line(&make_unknown_line("b", 60))
            .unwrap();
        // Upsert "c" once with score 90 — highest interest wins.
        storage
            .cache_unknown_line(&make_unknown_line("c", 90))
            .unwrap();

        let rows = storage.list_unknown_lines(50).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].shape_hash, "c");
        assert_eq!(rows[1].shape_hash, "b");
        assert_eq!(rows[2].shape_hash, "a");
    }
}
