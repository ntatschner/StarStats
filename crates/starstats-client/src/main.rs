//! StarStats tray client — Tauri 2 host process.
//!
//! Wiring:
//!   - SQLite store opened in user data dir
//!   - Game.log discovery → start tail loop on first match
//!   - Tray icon with Show / Quit
//!   - Tauri commands exposed to the React frontend

// Detach the console window in release builds — this is a tray app,
// users launching from the Start menu don't expect a flashing cmd
// window. Stdout/stderr are silenced as a side-effect; persistent
// diagnostics live in the panic hook + the optional `debug_logging`
// file appender (see `init_telemetry`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backfill;
mod commands;
mod config;
mod crashes;
mod discovery;
mod gamelog;
mod launcher;
// Tray-side hangar fetcher (Wave 5b). Spawned from the Tauri setup
// closure below when an api_url + access_token are configured.
mod hangar;
// Foundational layer for tray-side hangar fetching (Wave 5b).
// `process_guard` is consumed by `hangar` (kept here as a
// first-party module so the binary's trust scope is explicit);
// `secret` is consumed by both `hangar` and the cookie-management
// commands.
mod parser_defs;
#[allow(dead_code)]
mod process_guard;
mod secret;
mod state;
mod storage;
mod sync;

use crate::hangar::HangarStats;
use crate::state::{AccountStatus, AppState};
use crate::storage::Storage;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Registry};

fn main() {
    let debug_logging = config::load().map(|c| c.debug_logging).unwrap_or(false);
    init_telemetry(debug_logging);
    install_panic_hook();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "starstats-client starting"
    );

    tauri::Builder::default()
        // Intercept the main window's close button — without this,
        // closing the window destroys the only webview and Tauri's
        // default "exit on last window close" kicks in, killing the
        // app instead of leaving the tray icon resident. Hide instead;
        // Quit is reachable from the tray menu.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            // 0. Updater plugin — desktop only. Always registered so
            //    the manual "Check for updates" command in the Settings
            //    pane works regardless of the auto-check preference.
            //    The startup auto-check below is gated on the user's
            //    `auto_update_check` config flag.
            #[cfg(desktop)]
            {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;

                let auto_check = config::load().map(|c| c.auto_update_check).unwrap_or(true);
                if auto_check {
                    use tauri_plugin_updater::UpdaterExt;
                    let handle = app.handle().clone();
                    let current_version = env!("CARGO_PKG_VERSION");
                    tauri::async_runtime::spawn(async move {
                        match handle.updater() {
                            Ok(updater) => match updater.check().await {
                                Ok(Some(update)) => tracing::info!(
                                    current_version = current_version,
                                    new_version = %update.version,
                                    "starstats update available"
                                ),
                                Ok(None) => tracing::info!(
                                    current_version = current_version,
                                    "starstats is up to date"
                                ),
                                Err(e) => tracing::warn!(
                                    error = %e,
                                    current_version = current_version,
                                    "updater check failed"
                                ),
                            },
                            Err(e) => tracing::warn!(
                                error = %e,
                                current_version = current_version,
                                "could not get updater handle"
                            ),
                        }
                    });
                } else {
                    tracing::info!("startup updater check skipped (auto_update_check=false)");
                }
            }

            // 1. Local SQLite store
            let storage_path = config::data_dir()?.join("data.sqlite3");
            let storage = Arc::new(Storage::open(&storage_path)?);
            tracing::info!(path = %storage_path.display(), "opened local store");

            // Hydrate the parser-definition cache from sqlite before
            // any ingest spawns — guarantees the first events through
            // the tail benefit from any rules cached on the previous
            // run, even if the network fetch hasn't landed yet.
            let parser_def_cache = parser_defs::RuleCache::new();
            parser_defs::hydrate_from_storage(&storage, &parser_def_cache);
            // Spawn the network refresher on the Tauri runtime so it
            // doesn't need a local tokio context. 6h cadence; first
            // tick runs immediately so an online cold-start picks up
            // the active manifest.
            if let Some(api_url) = config::load()
                .ok()
                .and_then(|c| c.remote_sync.api_url.clone())
            {
                let storage_for_fetch = Arc::clone(&storage);
                let cache_for_fetch = parser_def_cache.clone();
                tauri::async_runtime::spawn(parser_defs::run_fetcher(
                    api_url,
                    storage_for_fetch,
                    cache_for_fetch,
                ));
            }

            // 2. Live tail stats holder
            let tail_stats = Arc::new(parking_lot::Mutex::new(gamelog::TailStats::default()));
            let sync_stats = Arc::new(parking_lot::Mutex::new(sync::SyncStats::default()));
            let hangar_stats: Arc<parking_lot::Mutex<HangarStats>> =
                Arc::new(parking_lot::Mutex::new(HangarStats::default()));
            let account_status = Arc::new(parking_lot::Mutex::new(AccountStatus::default()));
            let sync_kick = Arc::new(tokio::sync::Notify::new());

            // 2a/2b/2c. Sync worker + account-status hydration +
            //           hangar refresh worker.
            start_sync_workers(
                Arc::clone(&storage),
                Arc::clone(&sync_stats),
                Arc::clone(&hangar_stats),
                Arc::clone(&account_status),
                Arc::clone(&sync_kick),
            );

            // 3. Discover Game.log and start tailing the most recently
            //    modified one (LIVE if the user just played).
            let watcher = start_log_tail(
                Arc::clone(&storage),
                Arc::clone(&tail_stats),
                parser_def_cache.clone(),
            )?;

            // 3a/3b/3c. Background workers — launcher tail, crash-dir
            //     scanner, rotated-log backfill. Each is wrapped in
            //     `catch_unwind` so a panic in any one of them doesn't
            //     take down the whole app. Defense-in-depth — the
            //     panic hook still captures the trace before unwinding.
            let launcher_stats =
                Arc::new(parking_lot::Mutex::new(launcher::LauncherStats::default()));
            let launcher_storage = Arc::clone(&storage);
            let launcher_stats_clone = Arc::clone(&launcher_stats);
            let launcher_watcher = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || {
                    tauri::async_runtime::block_on(async move {
                        launcher::start_tail(launcher_storage, launcher_stats_clone).await
                    })
                },
            )) {
                Ok(Ok(w)) => w,
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "launcher tail start failed; continuing without it");
                    None
                }
                Err(_) => {
                    tracing::error!(
                        "launcher::start_tail PANICKED; continuing without launcher tail"
                    );
                    None
                }
            };

            let crash_stats = Arc::new(parking_lot::Mutex::new(crashes::CrashStats::default()));
            let crash_storage = Arc::clone(&storage);
            let crash_stats_clone = Arc::clone(&crash_stats);
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crashes::spawn_scanner(crash_storage, crash_stats_clone);
            }))
            .is_err()
            {
                tracing::error!(
                    "crashes::spawn_scanner PANICKED; continuing without crash scanning"
                );
            }

            let backfill_stats =
                Arc::new(parking_lot::Mutex::new(backfill::BackfillStats::default()));
            let backfill_storage = Arc::clone(&storage);
            let backfill_stats_clone = Arc::clone(&backfill_stats);
            let backfill_rules = parser_def_cache.clone();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                backfill::spawn(backfill_storage, backfill_stats_clone, backfill_rules);
            }))
            .is_err()
            {
                tracing::error!(
                    "backfill::spawn PANICKED; continuing without rotated-log backfill"
                );
            }

            app.manage(AppState {
                storage,
                tail_stats,
                sync_stats,
                hangar_stats,
                account_status,
                sync_kick,
                launcher_stats,
                crash_stats,
                backfill_stats,
                parser_def_cache,
                _tail_handle: parking_lot::Mutex::new(watcher),
                _launcher_handle: parking_lot::Mutex::new(launcher_watcher),
            });

            // 4. Tray icon + menu
            build_tray(app)?;

            // 5. Show the main window on first launch
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_config,
            commands::save_config,
            commands::get_discovered_logs,
            commands::get_parse_coverage,
            commands::get_session_timeline,
            commands::list_transactions,
            commands::get_app_version,
            commands::reparse_events,
            commands::get_source_stats,
            commands::get_storage_stats,
            commands::mark_event_as_noise,
            commands::pair_device,
            commands::refresh_account_info,
            commands::retry_sync_now,
            commands::set_rsi_cookie,
            commands::clear_rsi_cookie,
            commands::get_rsi_cookie_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show window", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &separator, &quit_item])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("tray icon".into()))?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("StarStats")
        .menu(&menu)
        // Left-click should not pop the menu — it should show the
        // window. Right-click still opens the menu (default platform
        // behavior).
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn init_telemetry(debug_logging: bool) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,starstats=info"));
    let stdout_layer = fmt::layer().with_target(false);

    // The daily-rolling file appender is opt-in. With debug_logging
    // off (the default for end users) we keep the user's data dir
    // tidy — no log accumulation. Toggle from Settings → Updates.
    // Panic.log is still written on panic, regardless of this flag,
    // so we never lose a crash trace.
    if debug_logging {
        if let Ok(dir) = config::data_dir() {
            let file_appender = tracing_appender::rolling::daily(&dir, "client.log");
            let file_layer = fmt::layer()
                .with_writer(file_appender)
                .with_target(false)
                .with_ansi(false);
            let _ = Registry::default()
                .with(filter)
                .with(stdout_layer)
                .with(file_layer)
                .try_init();
            return;
        }
    }
    let _ = Registry::default()
        .with(filter)
        .with(stdout_layer)
        .try_init();
}

/// Capture panics to a dedicated `panic.log` in the user data dir
/// using direct unbuffered writes — a panic during setup can exit
/// the process within milliseconds, faster than tracing's pipeline
/// can flush. The default panic hook still runs afterwards so debug
/// builds keep the standard stderr trace.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!("panic: {info}\nbacktrace:\n{backtrace}");

        if let Ok(dir) = config::data_dir() {
            let path = dir.join("panic.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write;
                let _ = writeln!(
                    f,
                    "[{}] [v{}] panic: {info}\nbacktrace:\n{backtrace}\n---",
                    chrono::Utc::now().to_rfc3339(),
                    env!("CARGO_PKG_VERSION"),
                );
            }
        }

        default_hook(info);
    }));
}

/// Spawns the background sync worker (no-op if `remote_sync.enabled`
/// is false), fires a one-shot `/v1/auth/me` hydration, and spawns
/// the hangar refresh worker — all fire-and-forget. The hangar worker
/// is gated on api_url + access_token being present (no point pushing
/// to a server we can't authenticate against); per-cycle decisions
/// (cookie present? game running?) are made inside the worker itself.
/// The UI shows a neutral account state until the hydration lands,
/// and the user can trigger a manual refresh via the
/// `refresh_account_info` command.
fn start_sync_workers(
    storage: Arc<Storage>,
    sync_stats: Arc<parking_lot::Mutex<sync::SyncStats>>,
    hangar_stats: Arc<parking_lot::Mutex<HangarStats>>,
    account_status: Arc<parking_lot::Mutex<AccountStatus>>,
    sync_kick: Arc<tokio::sync::Notify>,
) {
    let app_config = config::load().unwrap_or_default();

    let _sync_handle = sync::start(
        app_config.remote_sync.clone(),
        storage,
        sync_stats,
        Arc::clone(&account_status),
        sync_kick,
    );

    if let (Some(api_url), Some(token)) = (
        app_config.remote_sync.api_url.clone(),
        app_config.remote_sync.access_token.clone(),
    ) {
        // Hangar refresh worker — same auth posture as sync (needs
        // the device token + api_url). Skips per-cycle if the user
        // hasn't pasted an RSI cookie yet, or if the game is running.
        // Fire-and-forget — the JoinHandle drops with the runtime.
        let _hangar_handle = hangar::start(
            api_url.clone(),
            token.clone(),
            Arc::clone(&hangar_stats),
            Arc::clone(&account_status),
        );

        let account_status_for_init = Arc::clone(&account_status);
        tauri::async_runtime::spawn(async move {
            match sync::fetch_me(&api_url, &token).await {
                Ok(Some(me)) => {
                    let mut s = account_status_for_init.lock();
                    s.email_verified = Some(me.email_verified);
                    s.auth_lost = false;
                }
                Ok(None) => {
                    // Token rejected at startup — flip the banner so
                    // the user re-pairs before the next launch session.
                    tracing::warn!("startup /v1/auth/me rejected token — marking auth_lost");
                    let mut s = account_status_for_init.lock();
                    s.auth_lost = true;
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "startup /v1/auth/me failed — leaving account state neutral"
                    );
                }
            }
        });
    }
}

/// Picks the largest discovered live `Game.log` and starts tailing
/// it. Returns `Ok(None)` when no candidate is found in any standard
/// install path — the tray still launches in that case so the user
/// can pair a device or change the configured path.
///
/// Only `LogKind::ChannelLive` entries are considered. Discovery now
/// also surfaces archived rotated logs and crash reports for UI
/// visibility, but those aren't tail-able sources — picking one
/// would mean reading a stale file with no ongoing updates.
fn start_log_tail(
    storage: Arc<Storage>,
    tail_stats: Arc<parking_lot::Mutex<gamelog::TailStats>>,
    rules: parser_defs::RuleCache,
) -> anyhow::Result<Option<notify::RecommendedWatcher>> {
    let mut discovered: Vec<discovery::DiscoveredLog> = discovery::discover()
        .into_iter()
        .filter(|d| d.kind == discovery::LogKind::ChannelLive)
        .collect();
    discovered.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    let Some(log) = discovered.first().cloned() else {
        tracing::warn!("no live Game.log discovered in standard install paths");
        return Ok(None);
    };

    tracing::info!(
        channel = %log.channel,
        path = %log.path.display(),
        "starting tail"
    );
    {
        let mut s = tail_stats.lock();
        s.current_path = Some(log.path.clone());
    }
    let path = log.path.clone();
    let watcher = tauri::async_runtime::block_on(async move {
        gamelog::start_tail(path, storage, tail_stats, rules).await
    })?;
    Ok(Some(watcher))
}
