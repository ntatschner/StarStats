//! One-shot ingest of rotated `Game-*.log` files.
//!
//! At launch the engine renames the active `Game.log` to
//! `Logs/Game-YYYYMMDD-HHMMSS.log` and starts fresh. The live tailer
//! can't see those archives — it only watches the current Game.log.
//! This module sweeps every rotated archive on startup and replays
//! its contents through [`crate::gamelog::ingest_one_line`] so older
//! sessions land in the same store as the current one.
//!
//! Idempotency: each line's `(log_source, byte_offset, line)` triple
//! is the seed for the events table's UNIQUE key, so re-running the
//! backfill against the same file is safe — every duplicate row hits
//! ON CONFLICT DO NOTHING. The per-file cursor in `tail_cursors` is
//! also written at end-of-file, so the next pass short-circuits with
//! "nothing new" without reading a byte.
//!
//! Cost: rotated logs are typically 10–100 MB and parse in seconds.
//! We run the backfill on a separate tokio task so the tray UI shows
//! up immediately; the user sees backfilled events arrive as the
//! task finishes each file.

use crate::discovery::{self, LogKind};
use crate::gamelog::{ingest_one_line, log_source_from_path, IngestOutcome};
use crate::storage::Storage;
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};

/// Stats for the one-shot backfill, surfaced to the UI so the user
/// can see "we processed N rotated files at startup".
#[derive(Debug, Default, Clone, Serialize)]
pub struct BackfillStats {
    /// True once the initial sweep has finished. UI uses this to flip
    /// from "scanning archives…" to a final summary.
    pub completed: bool,
    /// Total rotated files discovered when the sweep started.
    pub files_total: u32,
    /// Files fully processed (cursor advanced to EOF). May lag
    /// `files_total` while the sweep is still running.
    pub files_processed: u32,
    /// Files we skipped because the cursor was already at EOF (i.e.
    /// a previous backfill run completed them).
    pub files_already_done: u32,
    /// Lines fed through `ingest_one_line` across every file.
    pub lines_processed: u64,
    /// Events that landed in the timeline (recognised by classify).
    pub events_recognised: u64,
}

/// Spawn the one-shot backfill on a background task. Returns
/// immediately so the rest of startup (tail watcher, sync worker)
/// isn't blocked. The task runs to completion and then exits.
///
/// Uses `tauri::async_runtime::spawn` rather than `tokio::spawn`:
/// the Tauri 2 setup closure runs synchronously on the main thread
/// without a tokio runtime in TLS, so a raw `tokio::spawn` panics
/// with "no reactor running". Tauri's wrapper queues onto the
/// runtime it owns.
pub fn spawn(storage: Arc<Storage>, stats: Arc<parking_lot::Mutex<BackfillStats>>) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_once(&storage, &stats).await {
            tracing::warn!(error = %e, "rotated-log backfill failed");
        }
        // Always flip `completed` so the UI exits its scanning state
        // even on partial failure — partial progress is still useful
        // and the user can re-launch to retry.
        stats.lock().completed = true;
    });
}

async fn run_once(storage: &Storage, stats: &parking_lot::Mutex<BackfillStats>) -> Result<()> {
    let all_discovered = discovery::discover();
    tracing::info!(
        total = all_discovered.len(),
        live = all_discovered
            .iter()
            .filter(|d| d.kind == LogKind::ChannelLive)
            .count(),
        archived = all_discovered
            .iter()
            .filter(|d| d.kind == LogKind::ChannelArchived)
            .count(),
        crash_report = all_discovered
            .iter()
            .filter(|d| d.kind == LogKind::CrashReport)
            .count(),
        launcher = all_discovered
            .iter()
            .filter(|d| d.kind == LogKind::LauncherLog)
            .count(),
        "backfill: discovery summary",
    );
    let archived: Vec<_> = all_discovered
        .into_iter()
        .filter(|d| d.kind == LogKind::ChannelArchived)
        .collect();

    for log in &archived {
        tracing::info!(
            path = %log.path.display(),
            channel = log.channel,
            size = log.size_bytes,
            "backfill: queued archived log",
        );
    }

    {
        let mut s = stats.lock();
        s.files_total = archived.len() as u32;
    }

    for log in archived {
        if let Err(e) = backfill_file(&log.path, storage, stats).await {
            tracing::warn!(
                path = %log.path.display(),
                error = %e,
                "backfill_file failed",
            );
        }
    }
    Ok(())
}

async fn backfill_file(
    path: &PathBuf,
    storage: &Storage,
    stats: &parking_lot::Mutex<BackfillStats>,
) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();
    let log_source = log_source_from_path(path);

    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // The file disappeared between discovery and open — count
            // it as "already done" so we don't block on it.
            stats.lock().files_already_done += 1;
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    let metadata = file.metadata().await?;

    let starting_offset = storage.read_cursor(&path_str)?;
    if starting_offset >= metadata.len() {
        // Previous backfill already drained this file. Skip without
        // reading any bytes — the cursor is the source of truth.
        stats.lock().files_already_done += 1;
        return Ok(());
    }

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(starting_offset)).await?;
    let mut offset = starting_offset;
    let mut buf = String::new();
    let mut local_lines = 0u64;
    let mut local_events = 0u64;

    loop {
        let line_start = offset;
        buf.clear();
        let n = reader.read_line(&mut buf).await?;
        if n == 0 {
            // EOF reached cleanly.
            break;
        }
        if !buf.ends_with('\n') {
            // Final partial line — rotated logs are closed/inactive
            // so a missing newline at the end is typically a truncated
            // last write. Stop here so a future re-run can pick it up
            // if the file gets fixed.
            break;
        }
        offset += n as u64;
        local_lines += 1;
        let outcome = ingest_one_line(
            buf.trim_end_matches(['\r', '\n']),
            storage,
            &log_source,
            line_start,
        );
        if matches!(outcome, IngestOutcome::Recognised { .. }) {
            local_events += 1;
        }
    }

    storage.write_cursor(&path_str, offset)?;

    tracing::info!(
        path = %path.display(),
        starting_offset,
        ending_offset = offset,
        lines = local_lines,
        events = local_events,
        "backfill: drained archived log",
    );

    let mut s = stats.lock();
    s.files_processed += 1;
    s.lines_processed = s.lines_processed.saturating_add(local_lines);
    s.events_recognised = s.events_recognised.saturating_add(local_events);
    Ok(())
}
