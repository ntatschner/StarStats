//! Tauri command surface — every function the JS frontend can call.
//!
//! Convention: errors stringify on the way out so the frontend gets a
//! human-readable message via the Tauri Promise rejection path.

use crate::backfill::BackfillStats;
use crate::config::{self, Config};
use crate::crashes::CrashStats;
use crate::discovery::{self, DiscoveredLog};
use crate::gamelog::TailStats;
use crate::hangar::HangarStats;
use crate::launcher::LauncherStats;
use crate::secret::{SecretStore, ACCOUNT_RSI_SESSION_COOKIE};
use crate::state::{AccountStatus, AppState};
use crate::sync::{self, SyncStats};
use serde::{Deserialize, Serialize};
use starstats_core::{
    apply_remote_rules, classify, pair_transactions, structural_parse, GameEvent, Transaction,
};
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct StatusResponse {
    pub tail: TailStats,
    pub sync: SyncStats,
    /// Hangar refresh worker's last-seen state (last attempt, last
    /// success, last error, ships pushed, last skip reason). Surfaced
    /// alongside `tail` and `sync` so the existing webview status-poll
    /// loop covers it without a dedicated command.
    pub hangar: HangarStats,
    pub event_counts: Vec<EventCount>,
    pub total_events: u64,
    pub discovered_logs: Vec<DiscoveredLog>,
    /// Account-lifecycle signals — `auth_lost` (device token rejected
    /// by the API) and `email_verified` (mirror of `GET /v1/auth/me`).
    /// Driven by the sync worker and the startup / post-pair refresh.
    pub account: AccountStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventCount {
    pub event_type: String,
    pub count: u64,
}

/// Coverage report for the parser — what's recognised, what's
/// structurally-known but unclassified, what's totally skipped, and
/// a list of the top unknowns the user could potentially write rules
/// for.
#[derive(Debug, Clone, Serialize)]
pub struct ParseCoverageResponse {
    pub recognised: u64,
    pub structural_only: u64,
    pub skipped: u64,
    /// Lines whose event_name was on the noise list — recognised as
    /// engine-internal chatter and dropped on purpose. Counted so the
    /// user sees "we filtered N noise lines" rather than wondering
    /// where they went.
    pub noise: u64,
    pub unknowns: Vec<UnknownSample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownSample {
    pub log_source: String,
    pub event_name: String,
    pub occurrences: u64,
    pub first_seen: String,
    pub last_seen: String,
    pub sample_line: String,
    pub sample_body: String,
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Result<StatusResponse, String> {
    let tail = state.tail_stats.lock().clone();
    let sync = state.sync_stats.lock().clone();
    let hangar = state.hangar_stats.lock().clone();
    let account = state.account_status.lock().clone();
    let counts = state
        .storage
        .event_counts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(event_type, count)| EventCount { event_type, count })
        .collect();
    let total = state.storage.total_events().map_err(|e| e.to_string())?;
    let discovered = discovery::discover();
    Ok(StatusResponse {
        tail,
        sync,
        hangar,
        event_counts: counts,
        total_events: total,
        discovered_logs: discovered,
        account,
    })
}

/// Re-fetch `GET /v1/auth/me` and update the in-memory account
/// snapshot. Called from the React side after a successful pair, and
/// once on startup. Returns the new `AccountStatus` so the caller can
/// reflect it immediately without a follow-up `get_status` round-trip.
///
/// On token absence (no pair yet) returns the current snapshot
/// unchanged. On 401/403 from the API, marks `auth_lost`. Network
/// errors are non-fatal — the snapshot keeps its previous value and
/// we surface the error string for the UI to optionally show.
#[tauri::command]
pub async fn refresh_account_info(state: State<'_, AppState>) -> Result<AccountStatus, String> {
    let cfg = config::load().map_err(|e| e.to_string())?;
    let (api_url, token) = match (
        cfg.remote_sync.api_url.as_deref(),
        cfg.remote_sync.access_token.as_deref(),
    ) {
        (Some(u), Some(t)) => (u.to_string(), t.to_string()),
        _ => return Ok(state.account_status.lock().clone()),
    };

    match sync::fetch_me(&api_url, &token).await {
        Ok(Some(me)) => {
            let mut s = state.account_status.lock();
            s.auth_lost = false;
            s.email_verified = Some(me.email_verified);
            Ok(s.clone())
        }
        Ok(None) => {
            // Server said the token is no longer valid. Treat the
            // same as the sync worker's auth-loss path so the UI
            // converges on a single state.
            let mut s = state.account_status.lock();
            s.auth_lost = true;
            Ok(s.clone())
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn get_config() -> Result<Config, String> {
    config::load().map_err(|e| e.to_string())
}

/// Outcome of a Rust-side updater check. Mirrors the JS `UpdateInfo`
/// type but with no opaque Update handle — the install path re-checks
/// internally because `tauri_plugin_updater::Update` isn't
/// Serializable and can't ride the IPC bridge.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckOutcome {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
}

fn build_channel_updater(
    app: &tauri::AppHandle,
    channel: crate::config::ReleaseChannel,
) -> Result<tauri_plugin_updater::Updater, String> {
    use tauri_plugin_updater::UpdaterExt;
    let url = channel
        .manifest_url()
        .parse::<tauri::Url>()
        .map_err(|e| format!("manifest URL did not parse: {e}"))?;
    let builder = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| format!("set endpoints: {e}"))?;
    builder.build().map_err(|e| format!("build updater: {e}"))
}

/// Check the given channel's manifest for a newer release.
///
/// We can't return the underlying `Update` handle to JS — its type
/// from `tauri-plugin-updater` isn't Serializable. Instead we return
/// just the metadata; the install command does its own check (the
/// race window is fine for our scale, and a new release between
/// check and install would simply install the newer one).
#[tauri::command]
pub async fn check_for_update_for_channel(
    channel: crate::config::ReleaseChannel,
    app: tauri::AppHandle,
) -> Result<UpdateCheckOutcome, String> {
    let updater = build_channel_updater(&app, channel)?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(u) => Ok(UpdateCheckOutcome {
            available: true,
            version: Some(u.version.clone()),
            notes: u.body.clone(),
            date: u.date.map(|d| d.to_string()),
        }),
        None => Ok(UpdateCheckOutcome {
            available: false,
            version: None,
            notes: None,
            date: None,
        }),
    }
}

/// Download + install the latest release on the given channel,
/// emitting `update-progress` events on the way through. The
/// frontend listens for these to drive its progress bar; on success
/// the process plugin's `relaunch()` swaps in the new binary, so
/// this command does not return.
///
/// If the manifest reports nothing newer (e.g. the user already
/// installed it via another path between check and install), this
/// returns `Ok(false)` so the UI can flip back to "up to date".
#[tauri::command]
pub async fn install_update_for_channel(
    channel: crate::config::ReleaseChannel,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    use tauri::Emitter;
    let updater = build_channel_updater(&app, channel)?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let app_for_progress = app.clone();
    let downloaded = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total = std::sync::Arc::new(parking_lot::Mutex::new(None::<u64>));
    let downloaded_for_chunk = std::sync::Arc::clone(&downloaded);
    let total_for_chunk = std::sync::Arc::clone(&total);
    update
        .download_and_install(
            move |chunk_len, content_length| {
                let mut total_lock = total_for_chunk.lock();
                if total_lock.is_none() {
                    *total_lock = content_length;
                }
                let cur = downloaded_for_chunk
                    .fetch_add(chunk_len as u64, std::sync::atomic::Ordering::Relaxed)
                    .saturating_add(chunk_len as u64);
                let _ = app_for_progress.emit(
                    "update-progress",
                    serde_json::json!({
                        "downloaded": cur,
                        "total": *total_lock,
                    }),
                );
            },
            || {
                // download_and_install fires this once the bytes
                // are on disk and the installer is about to run.
                // No-op — the UI already shows "installing" once
                // download completes.
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn save_config(cfg: Config) -> Result<(), String> {
    config::save(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_discovered_logs() -> Vec<DiscoveredLog> {
    discovery::discover()
}

/// Surface parser coverage to the tray UI: how many lines are
/// recognised, how many were structurally parsed but unclassified,
/// how many were skipped, plus the top 50 unknown event types so
/// the user can see which rules would unlock the most data.
#[tauri::command]
pub fn get_parse_coverage(state: State<'_, AppState>) -> Result<ParseCoverageResponse, String> {
    let stats = state.tail_stats.lock().clone();
    let rows = state
        .storage
        .recent_unknowns(50)
        .map_err(|e| e.to_string())?;
    let unknowns = rows
        .into_iter()
        .map(|r| UnknownSample {
            log_source: r.log_source,
            event_name: r.event_name,
            occurrences: r.occurrences,
            first_seen: r.first_seen,
            last_seen: r.last_seen,
            sample_line: r.sample_line,
            sample_body: r.sample_body,
        })
        .collect();
    Ok(ParseCoverageResponse {
        recognised: stats.events_recognised,
        structural_only: stats.lines_structural_only,
        skipped: stats.lines_skipped,
        noise: stats.lines_noise,
        unknowns,
    })
}

/// Mark an event_name as noise — the next tail drain stops sampling
/// it and the existing unknown sample is dropped immediately. Used by
/// the tray UI's "ignore this" button on the unknowns list.
#[tauri::command]
pub fn mark_event_as_noise(state: State<'_, AppState>, event_name: String) -> Result<(), String> {
    state
        .storage
        .add_noise(&event_name, "user")
        .map_err(|e| e.to_string())
}

// -- Session timeline ------------------------------------------------

/// One row in the player-visible "what happened" feed. The summary is
/// formatted server-side so the frontend stays a thin renderer; if we
/// want to localise later this is the single point we change.
///
/// `raw_line` is the original log line as captured from disk — surfaced
/// by the Logs pane's detail drawer for forensic inspection.
/// `log_source` is the channel tag (LIVE/PTU/EPTU) the event was tailed
/// from, displayed in the drawer's Source row.
/// `synced` is derived (not stored): an event is considered synced when
/// its id is at or below the persisted `sync_cursor.last_event_id`.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub id: i64,
    pub timestamp: String,
    pub event_type: String,
    pub summary: String,
    pub raw_line: String,
    pub log_source: String,
    pub synced: bool,
}

/// Default number of recent events surfaced when the caller doesn't
/// pass a `limit`. Tuned for the StatusPane glance view.
const DEFAULT_TIMELINE_LIMIT: usize = 50;

/// Hard cap on `limit`. Stops a frontend bug from asking for the
/// whole table over IPC — a typical row is ~500 bytes (raw line +
/// payload), so 5000 rows is a ~2.5 MB serialised response, which is
/// the largest we want to ship across the IPC boundary in one call.
const MAX_TIMELINE_LIMIT: usize = 5_000;

/// Clamp a caller-supplied limit into `[1, MAX_TIMELINE_LIMIT]`,
/// substituting the default when `None`. Pulled out of the Tauri
/// command so we can unit-test the bounds without spinning up an
/// AppState.
fn clamp_timeline_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_TIMELINE_LIMIT)
        .min(MAX_TIMELINE_LIMIT)
        .max(1)
}

#[tauri::command]
pub fn get_session_timeline(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<TimelineEntry>, String> {
    let limit = clamp_timeline_limit(limit);
    let rows = state
        .storage
        .recent_events(limit)
        .map_err(|e| e.to_string())?;
    // Snapshot the sync cursor once so all rows in this response are
    // judged against the same boundary — avoids the race where rows
    // straddle a cursor advance mid-iteration and the UI shows mixed
    // states for the same fetch.
    let cursor = state
        .storage
        .read_sync_cursor()
        .map_err(|e| e.to_string())?;

    let entries = rows
        .into_iter()
        .map(|r| {
            // Best-effort summary: if deserialisation fails we still
            // emit a row so the user sees something — they can drill
            // into the raw payload via the inspect tool.
            let summary = match serde_json::from_str::<GameEvent>(&r.payload_json) {
                Ok(event) => format_summary(&event),
                Err(_) => format!("{} (unparseable payload)", r.event_type),
            };
            let synced = r.id <= cursor;
            TimelineEntry {
                id: r.id,
                timestamp: r.timestamp,
                event_type: r.event_type,
                summary,
                raw_line: r.raw_line,
                log_source: r.log_source,
                synced,
            }
        })
        .collect();

    Ok(entries)
}

/// Aggregate the recent shop / commodity request-response pairs into
/// transaction rows. Pulls the last `limit` events, deserialises them,
/// hands the slice to `starstats_core::pair_transactions`, and returns
/// the resulting `Vec<Transaction>` to JS.
///
/// `window_secs` is the "if we haven't seen a response in N seconds,
/// mark it timed out" threshold. 30s is the default the UI uses; the
/// param exists so debugging can dial it down.
#[tauri::command]
pub fn list_transactions(
    state: State<'_, AppState>,
    limit: Option<usize>,
    window_secs: Option<i64>,
) -> Result<Vec<Transaction>, String> {
    let limit = clamp_timeline_limit(limit);
    let rows = state
        .storage
        .recent_events(limit)
        .map_err(|e| e.to_string())?;
    let events: Vec<GameEvent> = rows
        .into_iter()
        .filter_map(|r| serde_json::from_str::<GameEvent>(&r.payload_json).ok())
        .collect();
    // `now` for the ageing clock is the system time in UTC ISO. We
    // don't pull `chrono` here because we're already on it via the
    // workspace dep — `to_rfc3339()` matches the format the parser
    // emits.
    let now = chrono::Utc::now().to_rfc3339();
    let window = window_secs.unwrap_or(30);
    Ok(pair_transactions(&events, &now, window))
}

/// Aggregate counters surfaced by the Logs pane's headline strip:
/// how many events live in the local store and how big the on-disk
/// SQLite file currently is. Cheap to compute (two pragmas + a count)
/// and pulled on the same 10s cadence as the timeline.
#[derive(Debug, Clone, Serialize)]
pub struct StorageStats {
    pub total_events: u64,
    pub db_size_bytes: u64,
}

/// Combined snapshot of every secondary-source pipeline. Surfaced as
/// one command so the StatusPane can render a single "Sources" card
/// without three round-trips. Each sub-stats struct lives next to
/// its module — this is just the wire envelope.
#[derive(Debug, Clone, Serialize)]
pub struct SourceStats {
    pub launcher: LauncherStats,
    pub crashes: CrashStats,
    pub backfill: BackfillStats,
}

#[tauri::command]
pub fn get_source_stats(state: State<'_, AppState>) -> SourceStats {
    SourceStats {
        launcher: state.launcher_stats.lock().clone(),
        crashes: state.crash_stats.lock().clone(),
        backfill: state.backfill_stats.lock().clone(),
    }
}

/// Marketing-version string (Cargo.toml workspace version), surfaced
/// to the UI so the displayed version matches GitHub release tags
/// (e.g. "0.2.0-alpha"). This is distinct from Tauri's `getVersion()`
/// API, which returns the numeric `tauri.conf.json` version (MSI
/// bundlers reject non-numeric pre-release identifiers, so the Tauri
/// version is intentionally a numeric subset of the marketing one).
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Result of a re-parse pass over the local store.
#[derive(Debug, Clone, Serialize)]
pub struct ReparseStats {
    pub examined: u64,
    /// Rows whose `(type, payload)` changed because a newer/remote
    /// rule produced a different classification.
    pub updated: u64,
    /// Rows whose stored line no longer parses (probably mid-flight
    /// at capture time). Left untouched — never demoted.
    pub kept_unmatched: u64,
    /// Unknowns whose sample line now classifies. The first occurrence
    /// of each is promoted into `events`; the unknown row is removed.
    pub promoted_unknowns: u64,
    pub error: Option<String>,
}

/// Re-run the current classifier (built-ins + body-prefix + remote
/// rules) over every stored event line, in place. Existing rows are
/// updated when the new classification differs; otherwise left alone.
/// Idempotent — running it twice with the same rule set is a no-op
/// past the first.
///
/// Also walks `unknown_event_samples` and promotes any sample line
/// that the current classifier now recognises into a real `events`
/// row, removing the unknown record.
///
/// Heavy operation — async + spawn_blocking so the webview stays
/// responsive on a multi-million-row store.
#[tauri::command]
pub async fn reparse_events(state: State<'_, AppState>) -> Result<ReparseStats, String> {
    let storage = Arc::clone(&state.storage);
    let rules_snapshot = state.parser_def_cache.snapshot();

    tauri::async_runtime::spawn_blocking(move || run_reparse(&storage, &rules_snapshot))
        .await
        .map_err(|e| format!("reparse worker panicked: {e}"))?
}

fn run_reparse(
    storage: &crate::storage::Storage,
    rules: &[starstats_core::CompiledRemoteRule],
) -> Result<ReparseStats, String> {
    let mut stats = ReparseStats {
        examined: 0,
        updated: 0,
        kept_unmatched: 0,
        promoted_unknowns: 0,
        error: None,
    };

    // Phase 1 — re-classify already-recognised events. Also tracks
    // the most-recent zone signal as we walk so death events can be
    // back-filled with a best-effort `zone` field.
    //
    // Walk order is `id ASC` (per for_each_event), which matches
    // ingest order. For the typical workflow — live tail + on-startup
    // backfill of rotated logs — that approximates timestamp order
    // closely enough for the enrichment to land the right zone on
    // each death. Edge case: a late backfill that ingests OLDER logs
    // AFTER newer live-tail events would attribute the wrong zone
    // to those older deaths; users can re-run Re-parse after the
    // backfill catches up to fix it.
    let mut last_zone: Option<String> = None;
    let outcome = storage.for_each_event(500, |row| {
        stats.examined += 1;
        let Some(parsed) = structural_parse(&row.raw_line) else {
            stats.kept_unmatched += 1;
            return Ok(());
        };
        let Some(new_event) = classify(&parsed).or_else(|| apply_remote_rules(&parsed, rules))
        else {
            // The current rule set produces nothing for this line;
            // never demote — the row was recognised previously and
            // its stored payload is the best record we have.
            stats.kept_unmatched += 1;
            return Ok(());
        };

        // Update the zone tracker BEFORE enriching, so a death event
        // co-located with a fresh PlanetTerrainLoad on the same tick
        // doesn't accidentally pick up the older zone.
        match &new_event {
            GameEvent::PlanetTerrainLoad(t) => last_zone = Some(t.planet.clone()),
            GameEvent::LocationInventoryRequested(l) if l.location != "INVALID_LOCATION_ID" => {
                last_zone = Some(l.location.clone());
            }
            _ => {}
        }

        // Best-effort zone enrichment for death-related events.
        // Classify always returns `zone: None`; the enrichment pass
        // injects whatever last_zone has accumulated.
        let new_event = match new_event {
            GameEvent::PlayerDeath(mut d) if d.zone.is_none() => {
                d.zone = last_zone.clone();
                GameEvent::PlayerDeath(d)
            }
            GameEvent::PlayerIncapacitated(mut i) if i.zone.is_none() => {
                i.zone = last_zone.clone();
                GameEvent::PlayerIncapacitated(i)
            }
            other => other,
        };

        let Some((new_type, new_ts, new_payload)) = serialise_for_reparse(&new_event) else {
            stats.kept_unmatched += 1;
            return Ok(());
        };
        if new_type != row.event_type || new_ts != row.timestamp || new_payload != row.payload_json
        {
            storage
                .update_event_classification(row.id, &new_type, &new_ts, &new_payload)
                .map_err(|e| anyhow::anyhow!("update_event_classification: {e}"))?;
            stats.updated += 1;
        }
        Ok(())
    });
    if let Err(e) = outcome {
        stats.error = Some(format!("phase 1: {e}"));
        return Ok(stats);
    }

    // Phase 2 — promote unknowns whose stored sample now classifies.
    let unknowns = storage
        .recent_unknowns(usize::MAX)
        .map_err(|e| format!("recent_unknowns: {e}"))?;
    for sample in unknowns {
        let Some(parsed) = structural_parse(&sample.sample_line) else {
            continue;
        };
        let Some(new_event) = classify(&parsed).or_else(|| apply_remote_rules(&parsed, rules))
        else {
            continue;
        };
        let Some((new_type, new_ts, new_payload)) = serialise_for_reparse(&new_event) else {
            continue;
        };
        // We don't know the original byte offset for the unknown, so
        // synthesise a key keyed on the sample line itself. ON CONFLICT
        // DO NOTHING means a duplicate (same line in events already)
        // is silently skipped; success means a real promotion.
        let key = reparse_idempotency_key(&sample.log_source, &sample.sample_line);
        let insert_outcome = storage.insert_event(
            &key,
            &new_type,
            &new_ts,
            &sample.sample_line,
            &new_payload,
            &sample.log_source,
            0,
        );
        if let Err(e) = insert_outcome {
            tracing::warn!(error = %e, event = %sample.event_name, "promote unknown failed");
            continue;
        }
        // Remove the unknown sample regardless of whether the insert
        // was a fresh row or a no-op conflict — either way, the
        // sample is no longer a "next thing to write a rule for".
        if let Err(e) = storage.delete_unknown(&sample.log_source, &sample.event_name) {
            tracing::warn!(error = %e, "delete_unknown failed during reparse");
        }
        stats.promoted_unknowns += 1;
    }

    Ok(stats)
}

/// Outcome of `reingest_rotated_logs`. Mirrors the on-disk shape of
/// `BackfillStats` but without `completed`/`files_already_done` —
/// this command always re-walks every archived file from offset 0
/// regardless of saved cursor state, so those flags don't apply.
#[derive(Debug, Clone, Serialize)]
pub struct ReingestStats {
    pub files_walked: u32,
    pub files_failed: u32,
    pub lines_processed: u64,
    pub events_recognised: u64,
    /// Final non-fatal error message if the walk aborted partway.
    /// `None` on a clean run.
    pub error: Option<String>,
}

/// Forces a full re-walk of every rotated `Game-*.log` file, ignoring
/// the saved per-file cursor. Each line is fed back through
/// `ingest_one_line`, which dedupes already-known events via the
/// `(log_source, line_offset, line)` idempotency key — so previously-
/// classified rows stay where they are and only NEW classifications
/// (e.g. body-line PlayerDeath events under v0.3.2+ that were `None`'d
/// by an older parser) land fresh.
///
/// Side effect: `unknown_event_samples.occurrences` will inflate for
/// any event_name that still doesn't classify, because record_unknown
/// re-bumps the count on each pass. Acceptable noise — the goal is
/// recovering historical events that the modern parser now handles.
///
/// After the walk completes, the cursor is rewritten to EOF so the
/// next startup backfill short-circuits. The user typically clicks
/// Re-parse next to back-fill zone enrichment on the new rows.
#[tauri::command]
pub async fn reingest_rotated_logs(state: State<'_, AppState>) -> Result<ReingestStats, String> {
    let storage = Arc::clone(&state.storage);
    let rules_snapshot = state.parser_def_cache.snapshot();
    tauri::async_runtime::spawn_blocking(move || run_reingest(&storage, &rules_snapshot))
        .await
        .map_err(|e| format!("reingest worker panicked: {e}"))?
}

fn run_reingest(
    storage: &crate::storage::Storage,
    rules: &[starstats_core::CompiledRemoteRule],
) -> Result<ReingestStats, String> {
    use crate::discovery::{self, LogKind};
    use crate::gamelog::{ingest_one_line, log_source_from_path, IngestOutcome};
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let archived: Vec<_> = discovery::discover()
        .into_iter()
        .filter(|d| d.kind == LogKind::ChannelArchived)
        .collect();

    let mut stats = ReingestStats {
        files_walked: 0,
        files_failed: 0,
        lines_processed: 0,
        events_recognised: 0,
        error: None,
    };

    for log in archived {
        let path_str = log.path.to_string_lossy().to_string();
        let log_source = log_source_from_path(&log.path);

        let file = match File::open(&log.path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(
                    path = %log.path.display(),
                    error = %e,
                    "reingest: open failed",
                );
                stats.files_failed += 1;
                continue;
            }
        };

        let mut reader = BufReader::new(file);
        let mut offset: u64 = 0;
        let mut line_buf = String::new();
        let mut local_lines: u64 = 0;
        let mut local_events: u64 = 0;

        loop {
            let line_start = offset;
            line_buf.clear();
            let n = match reader.read_line(&mut line_buf) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(
                        path = %log.path.display(),
                        error = %e,
                        "reingest: read_line failed; stopping this file",
                    );
                    break;
                }
            };
            if n == 0 {
                break;
            }
            if !line_buf.ends_with('\n') {
                // Truncated final line — skip it like backfill does.
                break;
            }
            offset += n as u64;
            local_lines += 1;
            let outcome = ingest_one_line(
                line_buf.trim_end_matches(['\r', '\n']),
                storage,
                &log_source,
                line_start,
                rules,
            );
            if matches!(outcome, IngestOutcome::Recognised { .. }) {
                local_events += 1;
            }
        }

        // Park the cursor at EOF so the next startup backfill skips
        // this file. The whole point of this command is to bypass the
        // cursor, but ONLY for this run; subsequent startups should
        // resume the normal short-circuit path.
        if let Err(e) = storage.write_cursor(&path_str, offset) {
            tracing::warn!(
                path = %log.path.display(),
                error = %e,
                "reingest: write_cursor failed",
            );
        }
        stats.files_walked += 1;
        stats.lines_processed = stats.lines_processed.saturating_add(local_lines);
        stats.events_recognised = stats.events_recognised.saturating_add(local_events);
    }

    Ok(stats)
}

/// Mirror of `gamelog::serialise_event` but private to the reparse
/// path so tweaks here don't ripple into ingest. Returns
/// `(event_type, timestamp, payload_json)`.
fn serialise_for_reparse(event: &GameEvent) -> Option<(String, String, String)> {
    let payload = serde_json::to_string(event).ok()?;
    let value: serde_json::Value = serde_json::from_str(&payload).ok()?;
    let event_type = value.get("type")?.as_str()?.to_string();
    let timestamp = value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((event_type, timestamp, payload))
}

/// Stable key for an unknown-promoted-during-reparse row. Distinct
/// namespace (`reparse:`) so it can never collide with the live-tail
/// keyspace (`<source>:<offset>:<line>`) — same line + same source
/// produces the same key, so re-running reparse is idempotent.
fn reparse_idempotency_key(log_source: &str, line: &str) -> String {
    use uuid::Uuid;
    let payload = format!("reparse:{log_source}:{line}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, payload.as_bytes()).to_string()
}

#[tauri::command]
pub fn get_storage_stats(state: State<'_, AppState>) -> Result<StorageStats, String> {
    let total_events = state.storage.total_events().map_err(|e| e.to_string())?;
    let db_size_bytes = state
        .storage
        .database_size_bytes()
        .map_err(|e| e.to_string())?;
    Ok(StorageStats {
        total_events,
        db_size_bytes,
    })
}

/// Per-variant pretty rendering for the timeline. Kept exhaustive so
/// adding a new GameEvent variant fails to compile without an explicit
/// summary — the compiler is the safety net for "did we forget to
/// surface this in the UI".
fn format_summary(event: &GameEvent) -> String {
    match event {
        GameEvent::ProcessInit(_) => "Game process started".to_string(),
        GameEvent::LegacyLogin(e) => format!("Logged in as {}", e.handle),
        GameEvent::JoinPu(e) => format!("Joined PU shard {} ({}:{})", e.shard, e.address, e.port),
        GameEvent::ChangeServer(e) => format!(
            "Server transition: {}",
            match e.phase {
                starstats_core::ServerPhase::Start => "starting",
                starstats_core::ServerPhase::End => "complete",
            }
        ),
        GameEvent::SeedSolarSystem(e) => format!("Seeded {} on shard {}", e.solar_system, e.shard),
        GameEvent::ResolveSpawn(e) => format!(
            "Spawn resolved (player {}, fallback={})",
            e.player_geid, e.fallback
        ),
        GameEvent::ActorDeath(e) => format!(
            "{} killed by {} ({}, {})",
            e.victim, e.killer, e.weapon, e.damage_type
        ),
        GameEvent::PlayerDeath(e) => {
            // Strip the leading `body_` so the body class reads as
            // a recognisable variant name (e.g. `01_noMagicPocket`)
            // rather than redundant prefix noise.
            let class = e.body_class.strip_prefix("body_").unwrap_or(&e.body_class);
            match &e.zone {
                Some(z) => format!("Died ({class}) in {z}"),
                None => format!("Died ({class})"),
            }
        }
        GameEvent::PlayerIncapacitated(e) => match &e.zone {
            Some(z) => format!("Incapacitated in {z}"),
            None => "Incapacitated".to_string(),
        },
        GameEvent::VehicleDestruction(e) => format!(
            "Vehicle destroyed: {} (level {}, by {})",
            e.vehicle_class, e.destroy_level, e.caused_by
        ),
        GameEvent::HudNotification(e) => {
            // Trim the colon-space the engine pads onto banner text.
            let text = e.text.trim_end_matches(": ").trim_end_matches(':');
            format!("HUD: {text}")
        }
        GameEvent::LocationInventoryRequested(e) => {
            if e.location == "INVALID_LOCATION_ID" {
                format!("{} opened inventory (no location bound yet)", e.player)
            } else {
                format!("{} opened inventory at {}", e.player, e.location)
            }
        }
        GameEvent::PlanetTerrainLoad(e) => {
            // Strip the OOC_<system>_<key>_ prefix so we surface the
            // human-recognisable name (Daymar, Hurston, ArcCorp, etc.).
            let label = e.planet.rsplit('_').next().unwrap_or(&e.planet);
            format!("Near planet/moon: {label}")
        }
        GameEvent::QuantumTargetSelected(e) => {
            let phase = match e.phase {
                starstats_core::QuantumTargetPhase::FuelRequested => "fuel calc",
                starstats_core::QuantumTargetPhase::Selected => "selected",
            };
            format!(
                "Quantum target {phase}: {} → {}",
                e.vehicle_class, e.destination
            )
        }
        GameEvent::AttachmentReceived(e) => format!("Attached {} to {}", e.item_class, e.port),
        GameEvent::VehicleStowed(e) => {
            // Drop the `LandingArea_` / `[PROC]LandingArea_` prefix
            // so the surface area is readable.
            let area = e
                .landing_area
                .trim_start_matches("[PROC]")
                .trim_start_matches("LandingArea_");
            format!("Ship {} stowed at {}", e.vehicle_id, area)
        }
        GameEvent::GameCrash(e) => {
            // Use the dir name itself in the summary — it doubles as
            // a human-readable timestamp for crashes whose folder
            // followed the YYYY-MM-DD-HH-MM-SS convention.
            format!("Game crash ({}, {})", e.channel, e.crash_dir_name)
        }
        GameEvent::LauncherActivity(e) => {
            // Launcher messages are free-form. Truncate aggressively
            // for the timeline summary so a paragraph-long error
            // doesn't blow out the row height — the detail drawer
            // still surfaces the full body. The classified category
            // (auth/install/patch/...) leads so a glance shows what
            // the launcher is doing without reading the body.
            const SUMMARY_MAX: usize = 72;
            let truncated: String = e.message.chars().take(SUMMARY_MAX).collect();
            let suffix = if e.message.chars().count() > SUMMARY_MAX {
                "…"
            } else {
                ""
            };
            let category = match e.category {
                starstats_core::LauncherCategory::Auth => "AUTH",
                starstats_core::LauncherCategory::Install => "INSTALL",
                starstats_core::LauncherCategory::Patch => "PATCH",
                starstats_core::LauncherCategory::Update => "UPDATE",
                starstats_core::LauncherCategory::Error => "ERROR",
                starstats_core::LauncherCategory::Info => "INFO",
            };
            format!("[{category}] {truncated}{suffix}")
        }
        GameEvent::MissionStart(e) => {
            let kind = match e.marker_kind {
                starstats_core::MissionMarkerKind::Phase => "Mission accepted",
                starstats_core::MissionMarkerKind::Objective => "Mission objective",
            };
            // Mission name when the engine carried it; otherwise fall
            // back to the bare id so timeline rows stay distinguishable.
            let label = e.mission_name.as_deref().unwrap_or(&e.mission_id);
            format!("{kind}: {label}")
        }
        GameEvent::MissionEnd(e) => {
            // Outcome is best-effort; if missing, just record that the
            // mission terminated. Pair with a prior MissionStart for
            // duration if needed.
            match (&e.outcome, &e.mission_id) {
                (Some(o), _) => format!("Mission ended ({o})"),
                (None, Some(id)) => format!("Mission ended ({id})"),
                (None, None) => "Mission ended".to_string(),
            }
        }
        GameEvent::ShopBuyRequest(e) => match (&e.item_class, &e.quantity) {
            (Some(item), Some(qty)) => format!("Shop buy: {item} x{qty}"),
            (Some(item), None) => format!("Shop buy: {item}"),
            (None, _) => "Shop buy (pending)".to_string(),
        },
        GameEvent::ShopFlowResponse(e) => match e.success {
            Some(true) => "Shop purchase confirmed".to_string(),
            Some(false) => "Shop purchase rejected".to_string(),
            None => "Shop response".to_string(),
        },
        GameEvent::CommodityBuyRequest(e) => match (&e.commodity, &e.quantity) {
            (Some(c), Some(q)) => format!("Commodity buy: {c} ({q})"),
            (Some(c), None) => format!("Commodity buy: {c}"),
            (None, _) => "Commodity buy (pending)".to_string(),
        },
        GameEvent::CommoditySellRequest(e) => match (&e.commodity, &e.quantity) {
            (Some(c), Some(q)) => format!("Commodity sell: {c} ({q})"),
            (Some(c), None) => format!("Commodity sell: {c}"),
            (None, _) => "Commodity sell (pending)".to_string(),
        },
        GameEvent::SessionEnd(e) => match e.kind {
            starstats_core::SessionEndKind::SystemQuit => "Session ended (clean quit)".to_string(),
            starstats_core::SessionEndKind::FastShutdown => {
                "Session ended (fast shutdown)".to_string()
            }
        },
        GameEvent::RemoteMatch(e) => {
            // Show the rule's declared event name + a compact field
            // peek so the user can tell rules apart at a glance. We
            // don't try to reconstruct natural-language summaries —
            // rule authors don't know our format and we'd just be
            // making up text. The detail drawer renders fields fully.
            if e.fields.is_empty() {
                format!("[remote] {}", e.event_name)
            } else {
                let preview: Vec<String> = e
                    .fields
                    .iter()
                    .take(2)
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                format!("[remote] {} ({})", e.event_name, preview.join(", "))
            }
        }
    }
}

// -- Device pairing --------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PairOutcome {
    pub claimed_handle: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
struct RedeemResponseBody {
    token: String,
    label: String,
    /// Server-assigned UUID for this device pairing. Surfaced for
    /// future self-revoke + diagnostic logging — the tray captures
    /// it now (rather than ignoring the field) so we have it on
    /// disk if a later slice adds an "unpair this device" button.
    /// `#[allow(dead_code)]` until that slice lands; matches the
    /// pattern used for `RequireAdmin.0` in starstats-server.
    #[allow(dead_code)]
    device_id: uuid::Uuid,
}

/// Redeem an 8-character pairing code against the API and persist
/// the returned device JWT into the local config. Once this returns
/// success, the sync worker can drain queued events without further
/// user action.
///
/// The user's `claimed_handle` is decoded from the token's
/// `preferred_username` so it stays in sync with whatever the API
/// believes it should be — important if a future migration renames
/// handles.
#[tauri::command]
pub async fn pair_device(
    state: State<'_, AppState>,
    api_url: String,
    code: String,
) -> Result<PairOutcome, String> {
    let api_url = api_url.trim_end_matches('/').to_string();
    validate_pair_url(&api_url)?;
    let url = format!("{api_url}/v1/auth/devices/redeem");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;

    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "code": code.trim().to_uppercase() }))
        .send()
        .await
        .map_err(|e| format!("contact api: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("server returned {status}: {body}"));
    }

    let parsed: RedeemResponseBody = resp
        .json()
        .await
        .map_err(|e| format!("parse response: {e}"))?;

    let claimed_handle = decode_username_from_token(&parsed.token)
        .ok_or_else(|| "token did not contain preferred_username".to_string())?;

    // Persist into the local config — keeps the sync worker happy
    // and means the user doesn't have to re-enter anything.
    let mut cfg = config::load().map_err(|e| e.to_string())?;
    cfg.remote_sync.api_url = Some(api_url.clone());
    cfg.remote_sync.access_token = Some(parsed.token.clone());
    cfg.remote_sync.claimed_handle = Some(claimed_handle.clone());
    cfg.remote_sync.enabled = true;
    config::save(&cfg).map_err(|e| e.to_string())?;

    // Reset auth_lost — we just minted a fresh token. The running
    // sync worker (if any) was spawned with the old token and won't
    // pick up the new one until the next app start; that's acceptable
    // for now and matches the existing "Save settings → restart"
    // contract. Future: respawn the worker here.
    {
        let mut s = state.account_status.lock();
        s.auth_lost = false;
        s.email_verified = None;
    }

    // Best-effort: hydrate email_verified for the UI banner. If the
    // call fails (network blip), the banner just stays absent until
    // the next refresh — not worth failing the pair for.
    if let Ok(Some(me)) = sync::fetch_me(&api_url, &parsed.token).await {
        let mut s = state.account_status.lock();
        s.email_verified = Some(me.email_verified);
    }

    Ok(PairOutcome {
        claimed_handle,
        label: parsed.label,
    })
}

/// Reject pairing URLs that would leak the pairing code to a hostile
/// scheme. We allow `https://...` for production and `http://localhost`
/// (or `http://127.0.0.1`) for local development; everything else —
/// `javascript:`, `file:`, plain `http://example.com`, etc. — is
/// refused before the POST goes out.
fn validate_pair_url(api_url: &str) -> Result<(), String> {
    if let Some(rest) = api_url.strip_prefix("https://") {
        if rest.is_empty() {
            return Err("API URL must include a host".to_string());
        }
        return Ok(());
    }
    if let Some(rest) = api_url.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or("");
        let host_only = host.split(':').next().unwrap_or("");
        if host_only == "localhost" || host_only == "127.0.0.1" {
            return Ok(());
        }
        return Err("API URL must be https:// (http:// is only allowed for localhost)".to_string());
    }
    Err("API URL must start with https:// (or http://localhost for dev)".to_string())
}

/// Manually nudge the sync worker so the next batch ships without
/// waiting for the configured interval. No-op (still returns Ok) if
/// the worker isn't running — the user gets the same UX whether it
/// fires immediately or sits idle because remote sync is disabled.
#[tauri::command]
pub fn retry_sync_now(state: State<'_, AppState>) -> Result<(), String> {
    state.sync_kick.notify_one();
    Ok(())
}

/// Wake the hangar refresh worker immediately instead of waiting for
/// its next REFRESH_INTERVAL tick. Wired up by the Status pane's
/// "Refresh now" button. Silent no-op if the worker isn't spawned
/// (i.e. user hasn't paired their device yet) — the Notify just
/// queues a permit nobody consumes, costs nothing.
///
/// The cycle still respects per-tick gates (game running, no cookie
/// set, auth_lost) — kicking doesn't bypass safety, only the sleep.
#[tauri::command]
pub fn refresh_hangar_now(state: State<'_, AppState>) -> Result<(), String> {
    state.hangar_kick.notify_one();
    Ok(())
}

/// Pull `preferred_username` out of a JWT's payload without verifying
/// the signature — the server already verified it for us when it
/// minted the token. This is purely a UX convenience so we can show
/// the right handle on the next render.
fn decode_username_from_token(token: &str) -> Option<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("preferred_username")?.as_str().map(str::to_string)
}

// -- RSI session cookie management ----------------------------------
//
// The hangar fetcher (see `crate::hangar`) needs an authenticated
// RSI session cookie to read the user's pledge ledger. The user
// pastes that cookie value out of their browser DevTools; the tray
// stores it in the OS keychain via `SecretStore` and never displays
// it back. The three commands below — `set` / `clear` /
// `get_status` — are deliberately read-only with respect to the
// secret value itself: only a redacted preview ever leaves the host.

/// Upper bound on the cookie value length. The real RSI session
/// cookie is ~50–100 chars; 4096 is paranoid headroom that still
/// rejects accidental whole-page paste.
pub const MAX_COOKIE_CHARS: usize = 4096;

#[derive(Debug, Clone, Serialize)]
pub struct RsiCookieStatus {
    pub configured: bool,
    /// Last-4-character preview prefixed with an ellipsis (e.g.
    /// "…ab12"). Lets the user confirm "yes, I'm set up" without
    /// re-displaying the secret. `None` when no cookie is stored.
    pub preview: Option<String>,
}

/// Persist the user's pasted RSI session cookie value into the OS
/// keychain. Idempotent — overwrites any previous value. Returns the
/// redacted preview so the UI can confirm the write without echoing
/// the secret.
///
/// Takes `cookie_value` as a top-level Tauri command arg so the
/// plugin's automatic camelCase→snake_case mapping applies — JS
/// invokes with `{ cookieValue }` and Tauri rewrites the key for
/// us. The earlier `SetRsiCookieRequest { cookie_value }` wrapper
/// struct silently broke that mapping (Serde sees the inner field
/// names verbatim) and rejected the JS payload at runtime with
/// `missing field 'cookie_value'`.
#[tauri::command]
pub async fn set_rsi_cookie(cookie_value: String) -> Result<RsiCookieStatus, String> {
    let trimmed = cookie_value.trim();
    if trimmed.is_empty() {
        return Err("cookie value is empty".into());
    }
    if trimmed.chars().count() > MAX_COOKIE_CHARS {
        return Err("cookie value too long".into());
    }
    let store = SecretStore::new(ACCOUNT_RSI_SESSION_COOKIE).map_err(|e| e.to_string())?;
    store.set(trimmed).map_err(|e| e.to_string())?;
    Ok(RsiCookieStatus {
        configured: true,
        preview: Some(redact(trimmed)),
    })
}

/// Remove the stored cookie from the keychain. Idempotent — clearing
/// a missing entry is a no-op so the UI's "Forget cookie" path can
/// call this unconditionally.
#[tauri::command]
pub async fn clear_rsi_cookie() -> Result<RsiCookieStatus, String> {
    let store = SecretStore::new(ACCOUNT_RSI_SESSION_COOKIE).map_err(|e| e.to_string())?;
    store.clear().map_err(|e| e.to_string())?;
    Ok(RsiCookieStatus {
        configured: false,
        preview: None,
    })
}

/// Probe the keychain for the current RSI cookie status. Read-only —
/// returns just the redacted preview, never the secret.
#[tauri::command]
pub async fn get_rsi_cookie_status() -> Result<RsiCookieStatus, String> {
    let store = SecretStore::new(ACCOUNT_RSI_SESSION_COOKIE).map_err(|e| e.to_string())?;
    let stored = store.get().map_err(|e| e.to_string())?;
    let preview = stored.as_deref().map(redact);
    Ok(RsiCookieStatus {
        configured: stored.is_some(),
        preview,
    })
}

/// Build a redacted preview ("…XYZA") of a cookie value. Last four
/// characters are kept so the user can disambiguate two pastes from
/// the same browser without exposing meaningful prefix entropy.
fn redact(s: &str) -> String {
    let last4: String = s
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{last4}")
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_timeline_limit, redact, validate_pair_url, DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT,
    };

    #[test]
    fn clamp_timeline_limit_uses_default_when_none() {
        assert_eq!(clamp_timeline_limit(None), DEFAULT_TIMELINE_LIMIT);
    }

    #[test]
    fn clamp_timeline_limit_passes_through_in_range_value() {
        assert_eq!(clamp_timeline_limit(Some(1_000)), 1_000);
    }

    #[test]
    fn clamp_timeline_limit_caps_at_max() {
        assert_eq!(
            clamp_timeline_limit(Some(MAX_TIMELINE_LIMIT * 10)),
            MAX_TIMELINE_LIMIT
        );
    }

    #[test]
    fn clamp_timeline_limit_floor_is_one() {
        // Zero would produce an empty result silently — surface at
        // least one row so the caller can tell the table is non-empty.
        assert_eq!(clamp_timeline_limit(Some(0)), 1);
    }

    #[test]
    fn redact_keeps_last_four_chars() {
        assert_eq!(redact("abcdefghij"), "…ghij");
    }

    #[test]
    fn redact_handles_short_input() {
        // Fewer than four chars: just emit what's there. We never call
        // this on empty input (the command rejects empty before
        // redaction) so the "ellipsis only" case is fine.
        assert_eq!(redact("ab"), "…ab");
        assert_eq!(redact(""), "…");
    }

    #[test]
    fn redact_handles_unicode() {
        // Cookie values are ASCII in practice, but `chars` is
        // Unicode-aware so a multibyte tail won't slice mid-codepoint.
        assert_eq!(redact("hello🚀✨"), "…lo🚀✨");
    }

    #[test]
    fn validate_pair_url_accepts_https() {
        assert!(validate_pair_url("https://api.example.com").is_ok());
        assert!(validate_pair_url("https://api.example.com:8443/api").is_ok());
    }

    #[test]
    fn validate_pair_url_accepts_localhost_http() {
        assert!(validate_pair_url("http://localhost:3000").is_ok());
        assert!(validate_pair_url("http://127.0.0.1:8080").is_ok());
    }

    #[test]
    fn validate_pair_url_rejects_remote_http() {
        assert!(validate_pair_url("http://api.example.com").is_err());
        assert!(validate_pair_url("http://attacker.example/").is_err());
    }

    #[test]
    fn validate_pair_url_rejects_hostile_schemes() {
        assert!(validate_pair_url("javascript:alert(1)").is_err());
        assert!(validate_pair_url("file:///etc/passwd").is_err());
        assert!(validate_pair_url("data:text/html,<script>").is_err());
        assert!(validate_pair_url("").is_err());
    }

    #[test]
    fn validate_pair_url_rejects_https_without_host() {
        assert!(validate_pair_url("https://").is_err());
    }
}
