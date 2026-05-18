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
use starstats_core::templates::detect_bursts;
use starstats_core::{
    apply_remote_rules, classify, pair_transactions, structural_parse, BurstSummary, GameEvent,
    LogLine, Transaction,
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
    let mut cfg = config::load().map_err(|e| e.to_string())?;
    // Resolve web_origin server-side before the TS sees it. When the
    // user hasn't explicitly configured it, the derived value
    // (`api.<host>` → `<host>`) is what the "Open on web" affordance
    // should use. The on-disk config still stores None — we're only
    // hydrating the returned shape so the TS has a single value to
    // read instead of duplicating the resolution logic.
    if cfg.web_origin.is_none() {
        cfg.web_origin = cfg.effective_web_origin();
    }
    Ok(cfg)
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
pub fn save_config(state: State<'_, AppState>, cfg: Config) -> Result<(), String> {
    config::save(&cfg).map_err(|e| e.to_string())?;
    // Respawn the sync worker so a toggle of `remote_sync.enabled`
    // (or any other field — URL, token, batch size, etc.) takes
    // effect immediately instead of waiting for the next app start.
    // Idempotent: when the new config disables sync, respawn aborts
    // the old worker and leaves the handle as None.
    sync::respawn(
        Arc::clone(&state.storage),
        Arc::clone(&state.sync_stats),
        Arc::clone(&state.account_status),
        Arc::clone(&state.sync_kick),
        Arc::clone(&state.sync_handle),
    );
    Ok(())
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
        .clamp(1, MAX_TIMELINE_LIMIT)
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

/// How many entries the "Top event types" section is allowed to show.
/// Matches the spec for the clipboard summary — anything past 10 is
/// noise in a Discord paste.
const SESSION_SUMMARY_TOP_TYPES: usize = 10;

/// How many recent timeline rows the summary embeds. Caps the
/// clipboard payload at something hand-scannable.
const SESSION_SUMMARY_TIMELINE_LIMIT: usize = 20;

/// Column width for the event_type cell in the "Top event types"
/// section. Long enough for the longest classifier name we ship
/// without truncation, short enough that the count column stays close.
const SESSION_SUMMARY_TYPE_COL_WIDTH: usize = 22;

/// Column width for the event_type cell in the timeline section. The
/// timeline is denser than the top-types table so the column is
/// narrower; summaries flow into the remaining width.
const SESSION_SUMMARY_TIMELINE_TYPE_COL_WIDTH: usize = 15;

/// Insert a thousands separator into a u64 without pulling in a
/// formatting crate — the summary is the only place we need it.
fn format_count_with_commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

/// Extract `HH:MM` from an RFC3339 timestamp. Falls back to the raw
/// string's first 5 chars if parsing fails, so a malformed value still
/// produces *something* readable rather than the empty cell.
fn timeline_hhmm(raw: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(dt) => dt.with_timezone(&chrono::Utc).format("%H:%M").to_string(),
        Err(_) => raw.chars().take(5).collect(),
    }
}

/// Pure formatter for the clipboard-friendly session summary. Kept
/// free of any Tauri state so it can be unit-tested with fixture
/// slices and a pinned `now` instant.
///
/// Layout (sections separated by a blank line):
///   1. Title + "Generated <ts>" header
///   2. "Captured N events total" (or "No events captured yet." short-circuit)
///   3. Top event types (up to 10, padded columns, comma-separated counts)
///   4. Recent timeline (up to 20, HH:MM + padded type + summary)
fn format_session_summary(
    event_counts: &[EventCount],
    timeline: &[TimelineEntry],
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let header_ts = now.format("%Y-%m-%d %H:%M UTC").to_string();
    let total: u64 = event_counts.iter().map(|c| c.count).sum();

    // Empty-state short-circuit. Returning a tiny but still-useful
    // string keeps the clipboard action from looking broken when the
    // store is fresh.
    if total == 0 && timeline.is_empty() {
        return format!(
            "StarStats — session summary\nGenerated {header_ts}\n\nNo events captured yet.\n"
        );
    }

    let mut out = String::new();
    out.push_str("StarStats — session summary\n");
    out.push_str(&format!("Generated {header_ts}\n"));
    out.push('\n');
    out.push_str(&format!(
        "Captured {} events total\n",
        format_count_with_commas(total)
    ));

    if !event_counts.is_empty() {
        out.push('\n');
        out.push_str("Top event types:\n");
        for c in event_counts.iter().take(SESSION_SUMMARY_TOP_TYPES) {
            out.push_str(&format!(
                "  {:<width$}  {}\n",
                c.event_type,
                format_count_with_commas(c.count),
                width = SESSION_SUMMARY_TYPE_COL_WIDTH,
            ));
        }
    }

    if !timeline.is_empty() {
        out.push('\n');
        out.push_str(&format!(
            "Recent timeline (last {}):\n",
            SESSION_SUMMARY_TIMELINE_LIMIT.min(timeline.len())
        ));
        for entry in timeline.iter().take(SESSION_SUMMARY_TIMELINE_LIMIT) {
            out.push_str(&format!(
                "  {}  {:<width$}  {}\n",
                timeline_hhmm(&entry.timestamp),
                entry.event_type,
                entry.summary,
                width = SESSION_SUMMARY_TIMELINE_TYPE_COL_WIDTH,
            ));
        }
    }

    out
}

/// Build a plain-text summary of the current session suitable for
/// pasting into Discord, a forum post, or a bug report. Re-uses the
/// same accessors as `get_status` (event counts) and
/// `get_session_timeline` (recent rows) so the numbers always agree
/// with what the StatusPane is rendering.
///
/// Returns a `String` (not a struct) because the consumer is the
/// clipboard — keeping it pre-formatted on the Rust side avoids
/// scattering layout logic across the JS surface.
#[tauri::command]
pub async fn get_session_summary_text(state: State<'_, AppState>) -> Result<String, String> {
    let counts: Vec<EventCount> = state
        .storage
        .event_counts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(event_type, count)| EventCount { event_type, count })
        .collect();

    // Pull the same row set get_session_timeline returns, but cap the
    // fetch at the summary's own limit — there's no value spending
    // IPC bandwidth on rows we'd drop anyway.
    let rows = state
        .storage
        .recent_events(SESSION_SUMMARY_TIMELINE_LIMIT)
        .map_err(|e| e.to_string())?;
    let cursor = state
        .storage
        .read_sync_cursor()
        .map_err(|e| e.to_string())?;
    let timeline: Vec<TimelineEntry> = rows
        .into_iter()
        .map(|r| {
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

    Ok(format_session_summary(
        &counts,
        &timeline,
        chrono::Utc::now(),
    ))
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
    /// Bursts retroactively detected over already-stored events. Each
    /// hit produces one `burst_summary` row; the original member rows
    /// are deleted. Sessions already collapsed at live-tail time are a
    /// no-op here because the idempotency key matches the live shape.
    pub bursts_collapsed: u64,
    /// Total per-line member rows deleted as part of `bursts_collapsed`.
    /// Surfaced separately so the user can see the spam-reduction effect
    /// (a single burst commonly absorbs 20+ rows).
    pub members_suppressed: u64,
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
        bursts_collapsed: 0,
        members_suppressed: 0,
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
        //
        // TODO(Phase 3 follow-up): record per-field provenance for the
        // `zone` field via `starstats_core::provenance_for_inferred_field`
        // so the inference trail is visible alongside the value. Needs
        // the source `PlanetTerrainLoad` / `LocationInventoryRequested`
        // envelope's idempotency_key threaded through `last_zone`, which
        // is a wider refactor than the inference-engine wave.
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

    // Phase 3 — retro-burst detection. Walk each `log_source`'s history
    // in source-offset order, run `detect_bursts` over the
    // structural-parsed view, and replace matched runs with a single
    // synthetic `BurstSummary` row plus member deletions. The
    // idempotency key matches the live-tail format (UUIDv5 over
    // `log_source : anchor_offset : "{raw_line}|burst:{rule_id}:{size}"`)
    // so a session that was already collapsed live can never produce a
    // duplicate summary, and re-running this phase is a strict no-op
    // once the members have been deleted.
    let burst_rules = crate::burst_rules::builtin_burst_rules();
    let sources = match storage.distinct_log_sources() {
        Ok(s) => s,
        Err(e) => {
            stats.error = Some(format!("phase 3: distinct_log_sources: {e}"));
            return Ok(stats);
        }
    };
    for source in sources {
        let rows = match storage.events_for_burst_scan(&source) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, log_source = %source, "phase 3: events_for_burst_scan");
                continue;
            }
        };
        if rows.is_empty() {
            continue;
        }
        // Project to (parseable_index → row_idx) so detect_bursts sees a
        // contiguous LogLine stream without holes from corrupt or
        // truncated raw lines. Also skip already-collapsed
        // `burst_summary` rows from a previous pass — re-parsing them
        // would just no-op anyway, but skipping is cheaper than
        // re-running detect_bursts over them.
        let parsed: Vec<(usize, LogLine<'_>)> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.event_type != "burst_summary")
            .filter_map(|(idx, r)| structural_parse(&r.raw_line).map(|l| (idx, l)))
            .collect();
        if parsed.len() < 2 {
            continue;
        }
        let log_lines: Vec<LogLine<'_>> = parsed.iter().map(|(_, l)| l.clone()).collect();
        let hits = detect_bursts(&log_lines, &burst_rules);

        for hit in hits {
            // Map BurstHit indices (into `log_lines`) back to row
            // positions, then back to a BurstScanRow ref for the anchor
            // (the only one we need a stable id/offset/raw_line for —
            // the end position contributes only its timestamp via
            // `end_log` below).
            let anchor_row_idx = parsed[hit.start_index].0;
            let anchor_row = &rows[anchor_row_idx];
            let anchor_log = &log_lines[hit.start_index];
            let end_log = &log_lines[hit.end_index];
            let member_db_ids: Vec<i64> = hit
                .member_indices
                .iter()
                .map(|&i| rows[parsed[i].0].id)
                .collect();

            // Cap the anchor body before storing so a 20-page inventory
            // dump doesn't end up in the timeline payload. Matches the
            // 200-char cap in `process_buffer`.
            let sample: String = anchor_log.body.chars().take(200).collect();
            let summary = GameEvent::BurstSummary(BurstSummary {
                timestamp: anchor_log.timestamp.to_string(),
                rule_id: hit.rule_id.clone(),
                size: hit.size as u32,
                end_timestamp: end_log.timestamp.to_string(),
                anchor_body_sample: if sample.is_empty() {
                    None
                } else {
                    Some(sample)
                },
            });

            let Some((event_type, ts, payload)) = serialise_for_reparse(&summary) else {
                tracing::warn!(rule = %hit.rule_id, "phase 3: serialise BurstSummary");
                continue;
            };

            let synthetic_line =
                format!("{}|burst:{}:{}", anchor_row.raw_line, hit.rule_id, hit.size);
            let key = burst_idempotency_key(&source, anchor_row.source_offset, &synthetic_line);

            if let Err(e) = storage.insert_event(
                &key,
                &event_type,
                &ts,
                &anchor_row.raw_line,
                &payload,
                &source,
                anchor_row.source_offset,
            ) {
                tracing::warn!(error = %e, rule = %hit.rule_id, "phase 3: insert burst summary");
                continue;
            }

            // Delete each member row. The summary itself was inserted
            // under a fresh idempotency key (different from any
            // member's), so deleting members can never delete the
            // summary we just wrote.
            for id in &member_db_ids {
                match storage.delete_event_by_id(*id) {
                    Ok(n) => {
                        stats.members_suppressed = stats.members_suppressed.saturating_add(n as u64)
                    }
                    Err(e) => tracing::warn!(error = %e, id = id, "phase 3: delete member"),
                }
            }
            stats.bursts_collapsed += 1;
        }
    }

    Ok(stats)
}

/// Idempotency key for a retro-emitted burst summary. Same shape as
/// `gamelog::idempotency_key` (UUIDv5 over `source:offset:line`) so a
/// session that was already collapsed at live-tail time produces an
/// identical key and the `ON CONFLICT DO NOTHING` clause keeps the
/// existing row instead of inserting a duplicate.
fn burst_idempotency_key(log_source: &str, offset: u64, synthetic_line: &str) -> String {
    use uuid::Uuid;
    let payload = format!("{log_source}:{offset}:{synthetic_line}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, payload.as_bytes()).to_string()
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
        GameEvent::BurstSummary(e) => {
            // Friendlier rendering for the four built-in rules; falls
            // back to a generic "Burst: <id>" for anything else (e.g.
            // future remote-served rules).
            let label = match e.rule_id.as_str() {
                "loadout_restore_burst" => "Loadout restored",
                "terrain_load_burst" => "Terrain loaded",
                "hud_notification_burst" => "Notifications",
                "vehicle_stowed_burst" => "Vehicles stowed",
                _ => "Burst",
            };
            format!("{} ({} events)", label, e.size)
        }
        GameEvent::LocationChanged(e) => match &e.from {
            Some(from) => format!("Location: {} → {}", from, e.to),
            None => format!("Location: {}", e.to),
        },
        GameEvent::ShopRequestTimedOut(e) => match &e.item_class {
            Some(item) => format!(
                "Shop request timed out: {item} (after {}s)",
                e.timed_out_after_secs
            ),
            None => format!("Shop request timed out (after {}s)", e.timed_out_after_secs),
        },
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

    // Reset auth_lost — we just minted a fresh token. Order matters:
    // clear auth_lost BEFORE respawn so the new worker doesn't read
    // a stale `auth_lost = true` from the previous session and skip
    // its first drain.
    {
        let mut s = state.account_status.lock();
        s.auth_lost = false;
        s.email_verified = None;
    }

    // Respawn the sync worker with the just-persisted token. Previously
    // this required a tray restart — the worker spawned at boot with
    // `enabled = false` returned None and there was no mechanism to
    // start a fresh one. Mirrors the save_config respawn pattern.
    sync::respawn(
        Arc::clone(&state.storage),
        Arc::clone(&state.sync_stats),
        Arc::clone(&state.account_status),
        Arc::clone(&state.sync_kick),
        Arc::clone(&state.sync_handle),
    );

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

// === Health surface (added 2026-05-16) =================================

/// 60-second TTL cache for the two `sysinfo`-derived health inputs.
/// `get_health` is polled every 15s by the tray UI; constructing a
/// fresh `System` + `Disks` on each poll is individually cheap but
/// cumulatively wasteful when the tray idles for hours. The tuple is
/// `(stamped_at, sc_process_running, disk_free_bytes)`. We tolerate the
/// staleness — the SC-running and free-disk signals don't need
/// sub-minute resolution for the Health card to be useful.
static SYSINFO_CACHE: parking_lot::Mutex<Option<(std::time::Instant, bool, Option<u64>)>> =
    parking_lot::Mutex::new(None);

const SYSINFO_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// Returns `(sc_process_running, disk_free_bytes)` from the cache if
/// the entry is younger than `SYSINFO_TTL`, otherwise recomputes,
/// stores, and returns the fresh values. Lock is released before the
/// expensive recompute so a contended call doesn't serialize behind
/// the cache holder.
fn cached_sysinfo() -> (bool, Option<u64>) {
    let now = std::time::Instant::now();
    {
        let cache = SYSINFO_CACHE.lock();
        if let Some((stamped, sc, free)) = *cache {
            if now.duration_since(stamped) < SYSINFO_TTL {
                return (sc, free);
            }
        }
    }
    let (sc, free) = compute_sysinfo();
    *SYSINFO_CACHE.lock() = Some((now, sc, free));
    (sc, free)
}

/// Uncached `sysinfo` read used by `cached_sysinfo`. Constructs a
/// minimal `System` (processes only, no global memory/CPU refresh) and
/// queries the partition that hosts the StarStats data directory.
fn compute_sysinfo() -> (bool, Option<u64>) {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};
    let sys =
        System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
    let sc_running = sys
        .processes_by_name("StarCitizen.exe".as_ref())
        .next()
        .is_some()
        || sys
            .processes_by_name("StarCitizen".as_ref())
            .next()
            .is_some();
    let free = crate::config::data_dir()
        .ok()
        .and_then(|d| free_bytes_for_path(&d));
    (sc_running, free)
}

/// Assemble a `HealthInputs` snapshot from `AppState`, `Config`, the
/// secret store, and `sysinfo`. Pure read-only — never mutates state.
fn snapshot_health_inputs(state: &AppState) -> Result<crate::health::HealthInputs, String> {
    let now = chrono::Utc::now();

    let tail = state.tail_stats.lock().clone();
    let sync_snap = state.sync_stats.lock().clone();
    let hangar = state.hangar_stats.lock().clone();
    let account = state.account_status.lock().clone();
    let update_avail = state.update_available.lock().clone();

    let config = crate::config::load().map_err(|e| e.to_string())?;
    let gamelog_override_set = config.gamelog_path.is_some();
    let discovered = crate::discovery::discover();

    let cookie_configured = SecretStore::new(ACCOUNT_RSI_SESSION_COOKIE)
        .ok()
        .and_then(|s| s.get().ok())
        .flatten()
        .is_some();

    let (sc_process_running, disk_free_bytes) = cached_sysinfo();

    // Parse RFC3339 timestamps into DateTime<Utc>. A malformed value
    // disables the dependent check (e.g. GameLogStale), so log on
    // failure rather than silently swallow — without the warn, a
    // regression in the upstream timestamp shape would mask the
    // staleness signal indefinitely.
    let parse_dt = |label: &str, s: &Option<String>| -> Option<chrono::DateTime<chrono::Utc>> {
        let raw = s.as_deref()?;
        match chrono::DateTime::parse_from_rfc3339(raw) {
            Ok(d) => Some(d.with_timezone(&chrono::Utc)),
            Err(e) => {
                tracing::warn!(
                    field = label,
                    raw = raw,
                    error = %e,
                    "health snapshot: dropping malformed RFC3339 timestamp"
                );
                None
            }
        }
    };
    let tail_last_event_at = parse_dt("tail.last_event_at", &tail.last_event_at);
    let hangar_last_attempt_at = parse_dt("hangar.last_attempt_at", &hangar.last_attempt_at);
    let hangar_last_success_at = parse_dt("hangar.last_success_at", &hangar.last_success_at);

    Ok(crate::health::HealthInputs {
        now,
        gamelog_discovered_count: discovered.len(),
        gamelog_override_set,
        remote_sync_enabled: config.remote_sync.enabled,
        api_url: config.remote_sync.api_url.clone(),
        access_token: config.remote_sync.access_token.clone(),
        web_origin: config.web_origin.clone(),
        auth_lost: account.auth_lost,
        email_verified: account.email_verified,
        cookie_configured,
        sync_last_error: sync_snap.last_error.clone(),
        // The SyncStats type tracks per-attempt counters elsewhere; for
        // now this is left at zero. A future commit can plumb the
        // attempts-since-success counter through.
        sync_attempts_since_success: 0,
        hangar_last_attempt_at,
        hangar_last_success_at,
        hangar_last_skip_reason: hangar.last_skip_reason.clone(),
        tail_current_path: tail
            .current_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        tail_last_event_at,
        sc_process_running,
        disk_free_bytes,
        update_available_version: update_avail.as_ref().map(|u| u.version.clone()),
        dismissed: config.dismissed_health.clone(),
    })
}

/// Best-effort free-space query for the partition containing `path`.
/// Returns `None` on platforms or paths where it fails.
fn free_bytes_for_path(path: &std::path::Path) -> Option<u64> {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[tauri::command]
pub async fn get_health(
    state: State<'_, AppState>,
) -> Result<Vec<crate::health::HealthItem>, String> {
    let inputs = snapshot_health_inputs(&state)?;
    Ok(crate::health::current_health(&inputs))
}

/// Process-wide lock taken by `dismiss_health` (and any future
/// command that does load-mutate-save on `config.toml`) to prevent
/// the load+save race: two concurrent dismissals would otherwise
/// each load the pre-dismissal config, push their own item, then
/// the second write would clobber the first. The lock is module-
/// private; callers reach it only via the command surface.
static CONFIG_MUTATION_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[tauri::command]
pub async fn dismiss_health(
    id: crate::health::HealthId,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let inputs = snapshot_health_inputs(&state)?;
    let live = crate::health::current_health(&inputs);
    let target = live
        .iter()
        .find(|i| i.id == id)
        .ok_or_else(|| format!("No live HealthItem with id {:?}", id))?;
    if !target.dismissible {
        return Err(format!("HealthItem {:?} is not dismissible", id));
    }
    let _guard = CONFIG_MUTATION_LOCK.lock();
    let mut config = crate::config::load().map_err(|e| e.to_string())?;
    config
        .dismissed_health
        .push(crate::health::DismissedHealth {
            id: target.id,
            fingerprint: target.fingerprint.clone(),
            dismissed_at: chrono::Utc::now(),
        });
    crate::config::save(&config).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn check_api_url(url: String) -> Result<crate::probes::ApiUrlCheck, String> {
    Ok(crate::probes::check_api_url(url).await)
}

#[tauri::command]
pub async fn check_rsi_cookie(cookie: String) -> Result<crate::probes::CookieCheck, String> {
    Ok(crate::probes::check_rsi_cookie(cookie).await)
}

/// Set by `apps/tray-ui/src/updater.ts` after a successful auto-update
/// or manual update check that found a newer version. Feeds the
/// `HealthId::UpdateAvailable` item in the health surface.
///
/// Validates the version string: must look like semver-ish (digits,
/// dots, dashes, plus, ascii alphanumerics) and be at most 64 chars.
/// Renderer-controllable surface, so any compromise/bug in the JS
/// layer can't inject arbitrary text into the Health card via this
/// path.
#[tauri::command]
pub async fn set_update_available(
    version: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let version = version.trim();
    if version.is_empty() || version.len() > 64 {
        return Err("invalid version: must be 1-64 chars".into());
    }
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
    {
        return Err("invalid version: only alphanumerics, '.', '-', '+', '_' allowed".into());
    }
    *state.update_available.lock() = Some(crate::state::UpdateInfo {
        version: version.to_string(),
        checked_at: chrono::Utc::now(),
    });
    Ok(())
}

/// Default interest cutoff for the review pane and the badge count.
/// Matches the spec: only shapes scoring >= 50 surface to the user.
/// Lower scores stay cached for diagnostics but don't get promoted.
const UNKNOWN_LINE_REVIEW_THRESHOLD: u8 = 50;

/// Return the persisted per-install anonymous ID for parser submissions,
/// generating one on first call. Format: `anon_<uuid v4 simple>` (the
/// `anon_` prefix keeps the value visually distinguishable from device
/// IDs, batch IDs, etc. in logs without leaking install identity).
///
/// The server requires `client_anon_id` to be non-empty; this helper is
/// the only producer client-side, so empty values can never reach the
/// wire. Safe to call repeatedly — the second call onwards is a config
/// read.
fn get_or_create_client_anon_id() -> anyhow::Result<String> {
    let mut cfg = config::load()?;
    let (id, dirty) = resolve_client_anon_id(cfg.client_anon_id.as_deref());
    if dirty {
        cfg.client_anon_id = Some(id.clone());
        config::save(&cfg)?;
    }
    Ok(id)
}

/// Pure helper carved out for testability: given the persisted value
/// (if any), return `(id, dirty)` where `dirty == true` means the
/// caller should write the new id back to disk. Empty / whitespace
/// values are treated as missing so a corrupted config still self-heals.
fn resolve_client_anon_id(existing: Option<&str>) -> (String, bool) {
    if let Some(existing) = existing {
        if !existing.trim().is_empty() {
            return (existing.to_string(), false);
        }
    }
    (format!("anon_{}", uuid::Uuid::new_v4().simple()), true)
}

/// Tauri command exposing the stable per-install anon ID to the UI.
/// Today only used for diagnostics — the submission path generates and
/// injects the value server-side so the frontend can't impersonate
/// another install.
#[tauri::command]
pub fn client_anon_id() -> Result<String, String> {
    get_or_create_client_anon_id().map_err(|e| e.to_string())
}

/// List every non-dismissed unknown shape worth reviewing (score >=
/// `UNKNOWN_LINE_REVIEW_THRESHOLD`). Ordered by the storage layer:
/// interest desc, occurrence desc, last_seen desc.
#[tauri::command]
pub fn list_unknown_lines(
    state: State<'_, AppState>,
) -> Result<Vec<starstats_core::UnknownLine>, String> {
    state
        .storage
        .list_unknown_lines(UNKNOWN_LINE_REVIEW_THRESHOLD)
        .map_err(|e| e.to_string())
}

/// Cheap counter for the tray badge. Returns how many shapes are
/// currently above the review threshold and not dismissed.
#[tauri::command]
pub fn count_unknown_lines(state: State<'_, AppState>) -> Result<u32, String> {
    state
        .storage
        .count_unknown_lines(UNKNOWN_LINE_REVIEW_THRESHOLD)
        .map_err(|e| e.to_string())
}

/// Hide a shape from the review pane. The row stays in SQLite so a
/// future re-capture of the same shape doesn't re-promote it — the
/// user told us once they don't care.
#[tauri::command]
pub fn dismiss_unknown_line(state: State<'_, AppState>, shape_hash: String) -> Result<(), String> {
    state
        .storage
        .dismiss_unknown_line(&shape_hash)
        .map_err(|e| e.to_string())
}

/// Ship a batch of user-reviewed shapes to `POST /v1/parser-submissions`
/// on the configured server. On success, stamps `submitted_at` on each
/// shape row locally so the review pane stops surfacing them.
///
/// HTTP shape mirrors `sync::drain_once`: 30s timeout, bearer auth
/// against the persisted device token, 401/403 flips `auth_lost` and
/// bails. The cursor pattern doesn't apply — submissions are one-shot
/// from a user action, not a continuous drain.
#[tauri::command]
pub async fn submit_unknown_lines(
    state: State<'_, AppState>,
    payloads: Vec<starstats_core::ParserSubmission>,
) -> Result<starstats_core::wire::ParserSubmissionResponse, String> {
    if payloads.is_empty() {
        return Ok(starstats_core::wire::ParserSubmissionResponse {
            accepted: 0,
            deduped: 0,
            ids: Vec::new(),
        });
    }

    // Stamp the anon ID server-side rather than trusting whatever the
    // frontend put in `client_anon_id`. This both fixes the bug where
    // the UI was sending `""` (which the server rejects with 400) and
    // closes the impersonation gap where one install could submit
    // under another install's ID by editing the JS payload.
    let anon_id = get_or_create_client_anon_id().map_err(|e| e.to_string())?;
    let payloads: Vec<starstats_core::ParserSubmission> = payloads
        .into_iter()
        .map(|p| starstats_core::ParserSubmission {
            client_anon_id: anon_id.clone(),
            ..p
        })
        .collect();

    let cfg = config::load().map_err(|e| e.to_string())?;
    let api_url = cfg
        .remote_sync
        .api_url
        .clone()
        .ok_or_else(|| "remote sync not configured: api_url missing".to_string())?;
    let access_token = cfg
        .remote_sync
        .access_token
        .clone()
        .ok_or_else(|| "remote sync not configured: device not paired".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;

    let url = format!("{}/v1/parser-submissions", api_url.trim_end_matches('/'));
    let batch = starstats_core::wire::ParserSubmissionBatch {
        submissions: payloads.clone(),
    };
    let resp = client
        .post(&url)
        .bearer_auth(&access_token)
        .json(&batch)
        .send()
        .await
        .map_err(|e| format!("POST /v1/parser-submissions: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        // Mirror sync::drain_once — surface auth_lost so the existing
        // health banner picks it up. Submissions aren't critical
        // enough to clear the persisted token here; the next sync
        // drain will hit the same status and run that path.
        state.account_status.lock().auth_lost = true;
        return Err(format!("auth lost: parser-submissions returned {status}"));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("submissions failed: {status} {body}"));
    }

    let parsed: starstats_core::wire::ParserSubmissionResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse submissions response: {e}"))?;

    // Best-effort: stamp submitted_at locally so the review pane
    // hides the shapes the server accepted. We don't have per-row
    // accept/dedupe granularity in the response payload (the server
    // returns aggregate counts + ids), so we stamp every shape we
    // sent — the server already deduped server-side.
    let now = chrono::Utc::now().to_rfc3339();
    for p in &payloads {
        if let Err(e) = state.storage.mark_submitted(&p.shape_hash, &now) {
            tracing::warn!(
                shape_hash = %p.shape_hash,
                error = %e,
                "mark_submitted failed after successful POST",
            );
        }
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_timeline_limit, format_session_summary, redact, resolve_client_anon_id, run_reparse,
        validate_pair_url, EventCount, TimelineEntry, DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT,
    };
    use crate::storage::Storage;
    use tempfile::TempDir;

    /// Build a single TimelineEntry fixture for the session-summary
    /// formatter tests. Synced/raw_line/log_source aren't surfaced by
    /// the summary text, so they get throwaway placeholders.
    fn fixture_timeline_entry(
        id: i64,
        timestamp: &str,
        event_type: &str,
        summary: &str,
    ) -> TimelineEntry {
        TimelineEntry {
            id,
            timestamp: timestamp.to_string(),
            event_type: event_type.to_string(),
            summary: summary.to_string(),
            raw_line: String::new(),
            log_source: "LIVE".to_string(),
            synced: false,
        }
    }

    fn fixed_ts() -> chrono::DateTime<chrono::Utc> {
        // Pin to a known instant so tests don't depend on wall clock.
        // 2026-05-16 14:23:45 UTC -- matches the example in the spec.
        chrono::DateTime::parse_from_rfc3339("2026-05-16T14:23:45Z")
            .expect("parse fixed timestamp")
            .with_timezone(&chrono::Utc)
    }

    /// Phase 3 retro-burst end-to-end test. Seeds a fresh SQLite with a
    /// 5-line `AttachmentReceived` run (matches the
    /// `loadout_restore_burst` rule's min_burst_size of 3), runs
    /// `run_reparse`, and asserts the row count collapsed to 1
    /// `burst_summary` plus the expected stat fields.
    ///
    /// Re-running the same `run_reparse` over the post-collapse state
    /// must be a strict no-op (idempotency invariant).
    #[test]
    fn retro_burst_collapses_attachment_run() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("retro_burst.sqlite3");
        let storage = Storage::open(&path).expect("open storage");

        // Seed 5 AttachmentReceived lines + 1 unrelated line at the end
        // so we can verify the unrelated row survives. Use plausible
        // raw lines that `structural_parse` accepts and that the
        // `loadout_restore_burst` rule matches (event_name
        // `AttachmentReceived` + tag `Inventory`).
        let attachment_line = |i: u64| {
            format!(
                "<2026-05-10T12:00:0{}.000Z> [Notice] <AttachmentReceived> body_{} [Inventory]",
                i, i
            )
        };
        for i in 0..5u64 {
            let line = attachment_line(i);
            let key = format!("seed:LIVE:{}:{}", i * 100, line);
            let key = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, key.as_bytes()).to_string();
            storage
                .insert_event(
                    &key,
                    "attachment_received",
                    &format!("2026-05-10T12:00:0{}.000Z", i),
                    &line,
                    "{}",
                    "LIVE",
                    i * 100,
                )
                .expect("insert attachment");
        }
        let unrelated = "<2026-05-10T12:01:00.000Z> [Notice] <Join PU> address[1.2.3.4] port[1234] shard[pub_x_1_1] locationId[1] [Team_GameServices]";
        let key = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("seed:LIVE:9999:{}", unrelated).as_bytes(),
        )
        .to_string();
        storage
            .insert_event(
                &key,
                "join_pu",
                "2026-05-10T12:01:00.000Z",
                unrelated,
                "{}",
                "LIVE",
                9999,
            )
            .expect("insert unrelated");

        assert_eq!(storage.total_events().expect("count"), 6);

        // Pass empty remote rules — Phase 3 (retro-burst) is the only
        // path under test; built-in classification on the seed lines is
        // immaterial.
        let stats = run_reparse(&storage, &[]).expect("reparse");
        assert!(stats.error.is_none(), "reparse error: {:?}", stats.error);
        assert_eq!(stats.bursts_collapsed, 1, "expected one burst collapsed");
        assert_eq!(
            stats.members_suppressed, 5,
            "expected all 5 attachment rows suppressed"
        );

        // After collapse: 1 burst_summary + 1 unrelated event = 2 rows.
        assert_eq!(
            storage.total_events().expect("count"),
            2,
            "expected 5 attachments collapsed into 1 summary + 1 unrelated row"
        );

        // Idempotency: running again finds nothing new.
        let stats2 = run_reparse(&storage, &[]).expect("reparse #2");
        assert!(
            stats2.error.is_none(),
            "reparse #2 error: {:?}",
            stats2.error
        );
        assert_eq!(stats2.bursts_collapsed, 0, "second pass must be a no-op");
        assert_eq!(stats2.members_suppressed, 0);
        assert_eq!(storage.total_events().expect("count"), 2);
    }

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

    /// Two back-to-back `cached_sysinfo` calls land microseconds apart,
    /// well inside the 60s TTL. The cached path must return the exact
    /// same tuple — proves the cache is being read on the second call
    /// rather than recomputed.
    #[test]
    fn cached_sysinfo_hits_within_ttl() {
        let first = super::cached_sysinfo();
        let second = super::cached_sysinfo();
        assert_eq!(
            first, second,
            "cached call within TTL must return identical values"
        );
    }

    #[test]
    fn session_summary_empty_returns_no_events_line() {
        let out = format_session_summary(&[], &[], fixed_ts());
        assert!(
            out.contains("No events captured yet."),
            "empty summary should call out zero events, got:\n{out}"
        );
        assert!(out.starts_with("StarStats — session summary"));
    }

    #[test]
    fn session_summary_lists_top_types_in_order_and_count() {
        let counts = vec![
            EventCount {
                event_type: "login".to_string(),
                count: 234,
            },
            EventCount {
                event_type: "ship_destroyed".to_string(),
                count: 89,
            },
            EventCount {
                event_type: "location_enter".to_string(),
                count: 67,
            },
        ];
        let out = format_session_summary(&counts, &[], fixed_ts());
        // Order: login must appear before ship_destroyed which must
        // appear before location_enter.
        let login_idx = out.find("login").expect("login present");
        let ship_idx = out.find("ship_destroyed").expect("ship_destroyed present");
        let loc_idx = out.find("location_enter").expect("location_enter present");
        assert!(
            login_idx < ship_idx,
            "login should come before ship_destroyed"
        );
        assert!(
            ship_idx < loc_idx,
            "ship_destroyed should come before location_enter"
        );
        // Counts (comma-formatted) must show up.
        assert!(out.contains("234"), "count 234 missing: {out}");
        assert!(out.contains("89"), "count 89 missing: {out}");
        assert!(out.contains("67"), "count 67 missing: {out}");
    }

    #[test]
    fn session_summary_caps_top_types_at_ten() {
        let counts: Vec<EventCount> = (0..15)
            .map(|i| EventCount {
                event_type: format!("type_{:02}", i),
                count: (100 - i) as u64,
            })
            .collect();
        let out = format_session_summary(&counts, &[], fixed_ts());
        // First 10 should appear, indices 10..15 should not.
        for i in 0..10 {
            let name = format!("type_{:02}", i);
            assert!(out.contains(&name), "expected {name} in output: {out}");
        }
        for i in 10..15 {
            let name = format!("type_{:02}", i);
            assert!(
                !out.contains(&name),
                "did not expect {name} in capped output: {out}"
            );
        }
    }

    #[test]
    fn session_summary_caps_timeline_at_twenty() {
        // 25 entries, newest first (matches what storage::recent_events
        // returns). Each entry's summary embeds its index so we can
        // check which made the cut.
        let timeline: Vec<TimelineEntry> = (0..25)
            .map(|i| {
                fixture_timeline_entry(
                    i as i64,
                    "2026-05-16T14:00:00Z",
                    "test_event",
                    &format!("summary_{:02}", i),
                )
            })
            .collect();
        let counts = vec![EventCount {
            event_type: "test_event".to_string(),
            count: 25,
        }];
        let out = format_session_summary(&counts, &timeline, fixed_ts());
        // First 20 summaries (indices 0..20) must appear; 20..25 must not.
        for i in 0..20 {
            let s = format!("summary_{:02}", i);
            assert!(out.contains(&s), "expected {s} in timeline output: {out}");
        }
        for i in 20..25 {
            let s = format!("summary_{:02}", i);
            assert!(
                !out.contains(&s),
                "did not expect {s} in capped timeline: {out}"
            );
        }
    }

    #[test]
    fn session_summary_timestamp_header_present() {
        let out = format_session_summary(&[], &[], fixed_ts());
        assert!(
            out.starts_with("StarStats — session summary"),
            "must lead with the title, got: {out}"
        );
        // Find the "Generated " line and confirm a 4-digit year follows.
        let idx = out.find("Generated ").expect("Generated label");
        let tail = &out[idx + "Generated ".len()..];
        let year_part: String = tail.chars().take(4).collect();
        assert_eq!(
            year_part.len(),
            4,
            "expected at least 4 chars after 'Generated ', got: {tail}"
        );
        assert!(
            year_part.chars().all(|c| c.is_ascii_digit()),
            "first 4 chars after 'Generated ' should be a year, got: {year_part}"
        );
    }

    #[test]
    fn resolve_client_anon_id_returns_existing_when_set() {
        // Stable: the same persisted value comes back, no dirty flag,
        // no rewrite of config.toml on subsequent calls.
        let (id, dirty) = resolve_client_anon_id(Some("anon_existing_abc"));
        assert_eq!(id, "anon_existing_abc");
        assert!(!dirty, "existing id should not flag the config dirty");
    }

    #[test]
    fn resolve_client_anon_id_generates_when_missing() {
        // First-call path: produces a fresh anon_<uuid simple> string
        // and flags the config as dirty so the caller persists it.
        let (id, dirty) = resolve_client_anon_id(None);
        assert!(dirty, "missing id must flag the config dirty");
        assert!(
            id.starts_with("anon_"),
            "id should be prefixed with anon_, got: {id}"
        );
        // uuid simple is 32 hex chars.
        let hex = id.strip_prefix("anon_").unwrap();
        assert_eq!(hex.len(), 32, "expected 32-char uuid simple, got: {hex}");
        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit()),
            "id tail should be hex, got: {hex}"
        );
    }

    #[test]
    fn resolve_client_anon_id_regenerates_when_blank() {
        // A corrupted / blank config should self-heal rather than send
        // an empty string to the server (which the route rejects with
        // 400 `invalid_client_anon_id`).
        for blank in ["", "   ", "\n\t"] {
            let (id, dirty) = resolve_client_anon_id(Some(blank));
            assert!(dirty, "blank id ({blank:?}) must trigger regeneration");
            assert!(id.starts_with("anon_"), "regenerated id: {id}");
        }
    }
}
