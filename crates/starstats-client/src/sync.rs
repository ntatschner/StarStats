//! Background worker that drains locally-stored events to the
//! StarStats API.
//!
//! Loop:
//! 1. Read `sync_cursor.last_event_id`.
//! 2. Read up to `batch_size` events with `id > cursor`.
//! 3. POST as an [`IngestBatch`] with the configured bearer token.
//! 4. On success, advance the cursor to the highest id in the batch.
//! 5. Sleep `interval_secs` and repeat.
//!
//! Failures (network, 5xx) are logged and retried after the sleep —
//! the cursor is only advanced on a 2xx response, so events never get
//! lost.
//!
//! Auth invalidation (401/403) is treated specially: the worker clears
//! the persisted device token, flips `AccountStatus::auth_lost`, and
//! stops attempting upstream drains until the user re-pairs. The tail
//! loop keeps appending events to local SQLite throughout.

use crate::config;
use crate::state::AccountStatus;
use crate::storage::{Storage, UnsentEvent};
use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use starstats_core::wire::{EventEnvelope, IngestBatch, LogSource};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Debug, Deserialize)]
struct IngestResponse {
    #[allow(dead_code)]
    batch_id: String,
    accepted: u32,
    duplicate: u32,
    rejected: u32,
}

/// Shape returned by `GET /v1/auth/me`. Mirrors the server's
/// `auth_routes::MeResponse` — duplicated rather than depending on
/// the server crate to keep the tray's compile graph small.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MeResponse {
    pub user_id: String,
    pub email: String,
    pub claimed_handle: String,
    pub email_verified: bool,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct SyncStats {
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub batches_sent: u64,
    pub events_accepted: u64,
    pub events_duplicate: u64,
    pub events_rejected: u64,
}

/// Abort the currently-running sync worker (if any) and spawn a
/// fresh one with the current persisted config. Used by `save_config`
/// and `redeem_pair` to pick up new tokens / endpoints / enabled-flag
/// values without requiring an app restart.
///
/// Idempotent: calling it when the new config also fails to spawn a
/// worker (e.g. `enabled = false`) just leaves `sync_handle` as
/// `None`, which is the same state the boot path would produce.
///
/// Reads config from disk so the caller doesn't have to thread the
/// fresh config in — there's exactly one place that mutates it
/// (`config::save`), and it's always called before this helper.
pub fn respawn(
    storage: Arc<crate::storage::Storage>,
    sync_stats: Arc<parking_lot::Mutex<SyncStats>>,
    account_status: Arc<parking_lot::Mutex<crate::state::AccountStatus>>,
    sync_kick: Arc<Notify>,
    sync_handle: Arc<parking_lot::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
) {
    // Abort first so the old worker stops draining with stale auth
    // before we spawn a new one. `abort()` is non-blocking; the
    // tokio runtime cleans up at the next poll.
    if let Some(old) = sync_handle.lock().take() {
        old.abort();
        tracing::info!("sync: aborted previous worker");
    }

    let cfg = match crate::config::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "sync respawn: config load failed; leaving worker stopped");
            return;
        }
    };

    let new_handle = start(
        cfg.remote_sync.clone(),
        storage,
        sync_stats,
        account_status,
        sync_kick,
    );

    if new_handle.is_some() {
        tracing::info!(
            enabled = cfg.remote_sync.enabled,
            has_api_url = cfg.remote_sync.api_url.is_some(),
            "sync: spawned fresh worker"
        );
    } else {
        tracing::info!(
            enabled = cfg.remote_sync.enabled,
            "sync: config incomplete or disabled; no worker running"
        );
    }
    *sync_handle.lock() = new_handle;
}

/// Spawn the sync worker. The returned task handle drops with the
/// runtime; the worker runs forever (or until shutdown). Returns
/// `None` if remote sync is disabled or the config is incomplete.
///
/// `kick` lets the UI cut short the post-drain sleep on demand —
/// notifying it wakes the loop and triggers an immediate drain
/// attempt. Notifies that arrive while a drain is already in flight
/// are stored (one-shot) so the next sleep wakes immediately.
pub fn start(
    cfg: config::RemoteSyncConfig,
    storage: Arc<Storage>,
    sync_stats: Arc<parking_lot::Mutex<SyncStats>>,
    account_status: Arc<parking_lot::Mutex<AccountStatus>>,
    kick: Arc<Notify>,
) -> Option<tauri::async_runtime::JoinHandle<()>> {
    if !cfg.enabled {
        return None;
    }
    let api_url = cfg.api_url.clone()?;
    let claimed_handle = cfg.claimed_handle.clone()?;
    let access_token = cfg.access_token.clone()?;

    let interval = Duration::from_secs(cfg.interval_secs.max(5));
    let batch_size = cfg.batch_size.max(1);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .ok()?;

    Some(tauri::async_runtime::spawn(async move {
        loop {
            // If a previous iteration tripped auth_lost, skip the
            // drain entirely — the token has been cleared and we'd
            // just re-trigger the same 401. Wait for the user to
            // re-pair (which clears auth_lost and respawns this task).
            let auth_ok = !account_status.lock().auth_lost;
            if auth_ok {
                if let Err(e) = drain_once(
                    &client,
                    &api_url,
                    &access_token,
                    &claimed_handle,
                    &storage,
                    batch_size,
                    &sync_stats,
                    &account_status,
                )
                .await
                {
                    tracing::warn!(error = %e, "sync drain failed");
                    let mut s = sync_stats.lock();
                    s.last_error = Some(e.to_string());
                }
            }
            // Race the regular sleep against a manual kick. Whichever
            // fires first wins; the next iteration runs immediately.
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = kick.notified() => {}
            }
        }
    }))
}

#[allow(clippy::too_many_arguments)]
async fn drain_once(
    client: &reqwest::Client,
    api_url: &str,
    access_token: &str,
    claimed_handle: &str,
    storage: &Storage,
    batch_size: usize,
    sync_stats: &parking_lot::Mutex<SyncStats>,
    account_status: &parking_lot::Mutex<AccountStatus>,
) -> Result<()> {
    let cursor = storage.read_sync_cursor()?;
    let pending = storage.read_unsent(cursor, batch_size)?;
    if pending.is_empty() {
        return Ok(());
    }

    let highest_id = pending.iter().map(|e| e.id).max().unwrap_or(cursor);
    let batch = build_batch(claimed_handle, &pending);

    let url = format!("{}/v1/ingest", api_url.trim_end_matches('/'));
    {
        let mut s = sync_stats.lock();
        s.last_attempt_at = Some(now_rfc3339());
    }
    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&batch)
        .send()
        .await
        .context("POST /v1/ingest")?;

    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        // Auth invalidated — token was rejected (revoked device,
        // deleted account, signature invalid). Drop the stored token
        // so we don't keep re-trying with garbage, flip the UI flag,
        // and bail. The cursor stays put: the same batch will be
        // re-sent verbatim once the user re-pairs.
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(
            %status,
            body = %body,
            "ingest rejected device token — clearing and pausing sync"
        );
        if let Err(e) = clear_persisted_device_token() {
            tracing::warn!(error = %e, "failed to clear device token after auth loss");
        }
        {
            let mut s = account_status.lock();
            s.auth_lost = true;
        }
        anyhow::bail!("auth lost: ingest returned {status}");
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("ingest failed: {status} {body}");
    }

    let parsed: IngestResponse = resp.json().await.context("parse ingest response")?;

    storage.write_sync_cursor(highest_id)?;

    let mut s = sync_stats.lock();
    s.last_success_at = Some(now_rfc3339());
    s.last_error = None;
    s.batches_sent += 1;
    s.events_accepted += parsed.accepted as u64;
    s.events_duplicate += parsed.duplicate as u64;
    s.events_rejected += parsed.rejected as u64;
    Ok(())
}

/// Wipe the persisted device token + claimed_handle from the on-disk
/// config. `enabled` is left as-is — re-pairing will re-fill the
/// fields and resume the worker. Idempotent: safe to call when the
/// token is already absent.
fn clear_persisted_device_token() -> Result<()> {
    let mut cfg = config::load().context("load config to clear token")?;
    cfg.remote_sync.access_token = None;
    cfg.remote_sync.claimed_handle = None;
    config::save(&cfg).context("save config after clearing token")?;
    Ok(())
}

/// One-shot HTTP call to `GET /v1/auth/me`. Used on startup and after
/// pairing to populate the account status surface (email-verified
/// banner, future: avatar / display name).
///
/// Returns `Ok(None)` on 401/403 — auth was already lost; caller
/// should reflect that in `AccountStatus`. Returns `Err` on
/// network/5xx errors so the caller can decide whether to retry.
pub async fn fetch_me(api_url: &str, access_token: &str) -> Result<Option<MeResponse>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("build http client")?;

    let url = format!("{}/v1/auth/me", api_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .context("GET /v1/auth/me")?;

    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        tracing::warn!(%status, "GET /v1/auth/me rejected token");
        return Ok(None);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GET /v1/auth/me failed: {status} {body}");
    }
    let me: MeResponse = resp.json().await.context("parse MeResponse")?;
    Ok(Some(me))
}

fn build_batch(claimed_handle: &str, events: &[UnsentEvent]) -> IngestBatch {
    let envelopes: Vec<EventEnvelope> = events
        .iter()
        .map(|e| {
            // The locally-stored payload SHOULD always parse — we
            // wrote it ourselves on the parser side. If it doesn't
            // parse here, something has gone wrong (schema drift in
            // GameEvent, db corruption); log with idempotency key so
            // it's traceable, and ship `event: None` — the server
            // accepts the envelope and stores `event_type=unknown`,
            // which is at least visible in the unknown-events query
            // rather than silently lost.
            let event = match serde_json::from_str(&e.payload_json) {
                Ok(ev) => Some(ev),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        idempotency_key = %e.idempotency_key,
                        log_source = %e.log_source,
                        source_offset = e.source_offset,
                        "stored event payload failed to deserialize; shipping as null"
                    );
                    None
                }
            };
            EventEnvelope {
                idempotency_key: e.idempotency_key.clone(),
                raw_line: e.raw_line.clone(),
                event,
                source: parse_source(&e.log_source),
                source_offset: e.source_offset,
                // Per Phase 1.A: metadata stamping happens in a later
                // task; envelopes shipped today carry None and the
                // server back-fills observed metadata server-side.
                metadata: None,
            }
        })
        .collect();

    IngestBatch {
        schema_version: IngestBatch::CURRENT_SCHEMA_VERSION,
        batch_id: uuid::Uuid::now_v7().to_string(),
        game_build: None,
        claimed_handle: claimed_handle.to_string(),
        events: envelopes,
    }
}

fn parse_source(s: &str) -> LogSource {
    match s {
        "live" => LogSource::Live,
        "ptu" => LogSource::Ptu,
        "eptu" => LogSource::Eptu,
        "hotfix" => LogSource::Hotfix,
        "tech" => LogSource::Tech,
        _ => LogSource::Other,
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
