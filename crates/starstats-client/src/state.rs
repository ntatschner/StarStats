//! Application-wide shared state, surfaced to Tauri commands via
//! `tauri::State<AppState>`.

use crate::backfill::BackfillStats;
use crate::crashes::CrashStats;
use crate::gamelog::TailStats;
use crate::hangar::HangarStats;
use crate::launcher::LauncherStats;
use crate::storage::Storage;
use crate::sync::SyncStats;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Notify;

/// Snapshot of the most recent auto-update check, populated at
/// startup when `Config.auto_update_check` is true and also after a
/// manual "Check for updates" click. `None` while no check has run
/// yet or the latest check found no update. Surfaced to the UI via
/// `health::current_health` → `HealthId::UpdateAvailable`.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub checked_at: DateTime<Utc>,
}

/// Account-lifecycle signals the tray reflects in its UI.
///
/// `auth_lost` flips true when an upstream call returns 401/403 — the
/// device token was rejected (revoked, account deleted, signature
/// invalid). The sync worker pauses upstream drains until the user
/// re-pairs; the local tail keeps appending to SQLite as normal.
///
/// `email_verified` mirrors the value from `GET /v1/auth/me`. It is
/// `None` until we've successfully fetched it at least once. Cached in
/// memory only — not security-sensitive and re-fetched on every
/// startup / re-pair, so the durability cost isn't worth it.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AccountStatus {
    pub auth_lost: bool,
    pub email_verified: Option<bool>,
}

pub struct AppState {
    pub storage: Arc<Storage>,
    pub tail_stats: Arc<parking_lot::Mutex<TailStats>>,
    pub sync_stats: Arc<parking_lot::Mutex<SyncStats>>,
    pub hangar_stats: Arc<parking_lot::Mutex<HangarStats>>,
    pub account_status: Arc<parking_lot::Mutex<AccountStatus>>,
    /// Manual nudge to the sync worker — `notify_one()` cuts short
    /// the post-drain sleep so the next batch ships immediately.
    /// Wired up by the Logs pane's "Retry sync" button. Always
    /// allocated so commands can call into it regardless of whether
    /// the worker actually spawned this session.
    pub sync_kick: Arc<Notify>,
    /// Manual nudge to the hangar refresh worker. Same shape as
    /// `sync_kick` — `notify_one()` cuts short the inter-cycle
    /// sleep so a "Refresh now" button click triggers an immediate
    /// fetch instead of waiting up to REFRESH_INTERVAL. Always
    /// allocated; calling it when the worker isn't spawned (no
    /// api_url/token configured) is a silent no-op.
    pub hangar_kick: Arc<Notify>,
    /// Live counters from the launcher-log tailer. Optional shape
    /// because the tailer doesn't start when no launcher logs are
    /// found locally — `current_path = None` is the "not running"
    /// signal. Read by `commands::get_source_stats`.
    pub launcher_stats: Arc<parking_lot::Mutex<LauncherStats>>,
    /// Background crash-dir scanner stats. Read by
    /// `commands::get_source_stats`.
    pub crash_stats: Arc<parking_lot::Mutex<CrashStats>>,
    /// One-shot rotated-log backfill stats. The task runs to
    /// completion on startup; surfaces "scanning…" / final summary
    /// to the UI via `commands::get_source_stats`.
    pub backfill_stats: Arc<parking_lot::Mutex<BackfillStats>>,
    /// Runtime parser-rule cache. Held in AppState so the re-parse
    /// command can take a snapshot at the moment the user clicks the
    /// button — re-parsing should use the SAME rule set the live
    /// tail is using right then.
    pub parser_def_cache: crate::parser_defs::RuleCache,
    /// Handle to the running sync worker. Wrapped in a Mutex so the
    /// `save_config` and `redeem_pair` commands can abort it and spawn
    /// a fresh one when the user toggles `remote_sync.enabled` or
    /// pairs a new device — without requiring a full app restart.
    /// `None` when sync is disabled or the config is incomplete.
    pub sync_handle: Arc<parking_lot::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    /// Held for its lifetime — drop ends filesystem watching.
    pub _tail_handle: parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
    /// Same as `_tail_handle` but for the launcher-log watcher.
    pub _launcher_handle: parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
    /// Result of the most recent update check. `Some` when a newer
    /// version is available; cleared/set fresh on each check. Read
    /// by the health surface (`HealthId::UpdateAvailable`); written
    /// by the JS-side `updater.ts` after a successful check via the
    /// `set_update_available` Tauri command.
    pub update_available: Arc<parking_lot::Mutex<Option<UpdateInfo>>>,
}
