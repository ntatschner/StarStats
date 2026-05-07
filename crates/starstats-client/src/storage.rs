//! Local SQLite event store. Wraps `rusqlite` behind a `Mutex` so the
//! tail loop and Tauri command handlers can share it.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

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
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut last_id: i64 = 0;
        loop {
            let mut stmt = conn.prepare(
                "SELECT id, idempotency_key, type, timestamp, raw, payload, log_source, source_offset
                 FROM events
                 WHERE id > ?
                 ORDER BY id ASC
                 LIMIT ?",
            )?;
            let rows: Vec<EventRow> = stmt
                .query_map(params![last_id, batch_size as i64], |row| {
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
                })?
                .filter_map(|r| r.ok())
                .collect();
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
}
