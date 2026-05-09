//! Resumable `Game.log` tailer.
//!
//! Two layers:
//! - `notify::RecommendedWatcher` fires on every filesystem change to
//!   the file. We translate that into a "drain pending bytes" signal.
//! - A `tokio` task seeks to the saved byte offset, reads complete
//!   lines, parses them via `starstats-core`, and stores recognised
//!   events in SQLite.
//!
//! Truncation handling: at game launch the file is rotated. We detect
//! this by `metadata.len() < offset` and reset to `0`.

use crate::burst_rules::builtin_burst_rules;
use crate::parser_defs::RuleCache;
use crate::storage::Storage;
use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use starstats_core::templates::{detect_bursts, BurstRule};
use starstats_core::{
    apply_remote_rules, classify, structural_parse, BurstSummary, GameEvent, LogLine,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};
use tokio::sync::mpsc;

/// Live counters surfaced to the frontend.
#[derive(Debug, Default, Clone, Serialize)]
pub struct TailStats {
    pub current_path: Option<PathBuf>,
    pub bytes_read: u64,
    pub lines_processed: u64,
    pub events_recognised: u64,
    pub last_event_at: Option<String>,
    pub last_event_type: Option<String>,
    /// Lines that produced a `LogLine` (timestamp + body) but for
    /// which `classify` returned `None`. These are the actionable
    /// "we should write a parser rule for this" cases.
    pub lines_structural_only: u64,
    /// Lines the structural parser couldn't handle at all — banners,
    /// blanks, continuation lines, etc. Not actionable as parser rules.
    pub lines_skipped: u64,
    /// Lines whose event_name was on the noise list — we recognised
    /// them as engine-internal chatter and dropped them on purpose.
    /// Counted separately so the user can see "we filtered N noise
    /// lines" rather than silently hiding them.
    pub lines_noise: u64,
}

/// Start watching `path` and tailing its appended bytes. Returns the
/// watcher handle — drop it to stop watching.
pub async fn start_tail(
    path: PathBuf,
    storage: Arc<Storage>,
    stats: Arc<parking_lot::Mutex<TailStats>>,
    rules: RuleCache,
) -> Result<RecommendedWatcher> {
    let (tx, mut rx) = mpsc::channel::<()>(64);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if matches!(
                ev.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
            ) {
                let _ = tx.try_send(());
            }
        }
    })?;

    watcher.watch(&path, RecursiveMode::NonRecursive)?;

    let path_str = path.to_string_lossy().to_string();
    let mut offset = storage.read_cursor(&path_str)?;

    let path_clone = path.clone();
    let path_str_clone = path_str.clone();
    let storage_clone = Arc::clone(&storage);
    let stats_clone = Arc::clone(&stats);

    let rules_clone = rules.clone();
    tokio::spawn(async move {
        // Initial drain in case the file already has new data we haven't seen.
        if let Err(e) = drain(
            &path_clone,
            &path_str_clone,
            &mut offset,
            &storage_clone,
            &stats_clone,
            &rules_clone,
        )
        .await
        {
            tracing::warn!(error = %e, "initial tail drain failed");
        }

        while rx.recv().await.is_some() {
            // Coalesce bursts of filesystem events.
            while rx.try_recv().is_ok() {}
            if let Err(e) = drain(
                &path_clone,
                &path_str_clone,
                &mut offset,
                &storage_clone,
                &stats_clone,
                &rules_clone,
            )
            .await
            {
                tracing::warn!(error = %e, "tail drain failed; backing off");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });

    Ok(watcher)
}

async fn drain(
    path: &PathBuf,
    path_str: &str,
    offset: &mut u64,
    storage: &Storage,
    stats: &parking_lot::Mutex<TailStats>,
    rules: &RuleCache,
) -> Result<()> {
    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let metadata = file.metadata().await?;

    if metadata.len() < *offset {
        // Game launched, log rotated. Reset.
        tracing::info!(
            previous = *offset,
            current = metadata.len(),
            "log truncated — resetting offset"
        );
        *offset = 0;
    }
    if metadata.len() == *offset {
        // Nothing new.
        return Ok(());
    }

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(*offset)).await?;
    let mut buf = String::new();

    let log_source = log_source_from_path(path);

    let rules_snapshot = rules.snapshot();
    let burst_rules = builtin_burst_rules();

    // Accumulate this drain's complete lines into a buffer so the
    // burst matcher can see the run as a unit. Per-line ingest can't
    // detect a burst because the first AttachmentReceived is already
    // committed to storage by the time the second one arrives;
    // batching at drain boundaries solves that without needing a
    // cross-drain rolling buffer (live tails fire on every filesystem
    // notify, so a burst that lands in one fsync arrives in one
    // drain in practice).
    let mut buffered_lines: Vec<(String, u64)> = Vec::new();
    loop {
        let line_start = *offset;
        buf.clear();
        let n = reader.read_line(&mut buf).await?;
        if n == 0 {
            break;
        }
        if !buf.ends_with('\n') {
            // Partial line — leave for next drain.
            break;
        }
        *offset += n as u64;

        let trimmed = buf.trim_end_matches(['\r', '\n']).to_string();
        buffered_lines.push((trimmed, line_start));
    }

    if !buffered_lines.is_empty() {
        process_buffer(
            &buffered_lines,
            storage,
            stats,
            &log_source,
            &rules_snapshot,
            &burst_rules,
        );
        let mut s = stats.lock();
        s.bytes_read = *offset;
        s.lines_processed += buffered_lines.len() as u64;
    }

    storage.write_cursor(path_str, *offset)?;
    Ok(())
}

/// Process a batch of lines from one drain. Three passes:
///   1. Structural-parse every line into a `LogLine` (or `None`).
///   2. Run [`detect_bursts`] over the parseable subset and translate
///      member indices back to buffer positions; insert one
///      [`BurstSummary`] per hit.
///   3. Per-line classify+ingest for every line that wasn't claimed
///      by a burst.
///
/// Bursts dedupe against re-drains via an offset-based idempotency
/// key (the anchor line offset + the rule id + size), so a tray
/// crash mid-flush re-emits the same summary on retry rather than
/// producing duplicates.
fn process_buffer(
    buffer: &[(String, u64)],
    storage: &Storage,
    stats: &parking_lot::Mutex<TailStats>,
    log_source: &str,
    remote_rules: &[starstats_core::CompiledRemoteRule],
    burst_rules: &[BurstRule],
) {
    // Pass 1: parse every line. The Vec<Option<LogLine>> preserves
    // index alignment with `buffer` so we can map burst indices back
    // to buffer positions in pass 2.
    let parsed: Vec<Option<LogLine<'_>>> = buffer
        .iter()
        .map(|(line, _)| structural_parse(line))
        .collect();

    // Project to the parseable subset (with their original buffer
    // indices) so the matcher sees a contiguous slice of LogLines.
    let valid: Vec<(usize, LogLine<'_>)> = parsed
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.as_ref().map(|line| (i, line.clone())))
        .collect();
    let valid_lines: Vec<LogLine<'_>> = valid.iter().map(|(_, l)| l.clone()).collect();

    // Pass 2: burst detection. Indices are into `valid_lines`.
    let bursts = detect_bursts(&valid_lines, burst_rules);

    // Map burst-member indices back to original buffer indices for
    // suppression in pass 3.
    let mut suppressed: HashSet<usize> = HashSet::new();
    for burst in &bursts {
        for &valid_idx in &burst.member_indices {
            suppressed.insert(valid[valid_idx].0);
        }
    }

    // Pass 2b: emit one BurstSummary per burst.
    for burst in &bursts {
        let anchor_buf_idx = valid[burst.start_index].0;
        let end_buf_idx = valid[burst.end_index].0;
        let anchor_log = &valid_lines[burst.start_index];
        let end_log = &valid_lines[burst.end_index];
        let anchor_line = &buffer[anchor_buf_idx].0;
        let anchor_offset = buffer[anchor_buf_idx].1;
        // The anchor body can be huge (full inventory dump); cap
        // before storing so the timeline doesn't carry kilobytes per
        // burst summary.
        let sample: String = anchor_log.body.chars().take(200).collect();
        let summary = GameEvent::BurstSummary(BurstSummary {
            timestamp: anchor_log.timestamp.to_string(),
            rule_id: burst.rule_id.clone(),
            size: burst.size as u32,
            end_timestamp: end_log.timestamp.to_string(),
            anchor_body_sample: if sample.is_empty() {
                None
            } else {
                Some(sample)
            },
        });

        let Some((event_type, ts, payload)) = serialise_event(&summary) else {
            tracing::warn!(rule = %burst.rule_id, "burst summary failed to serialise");
            continue;
        };
        // Synthetic key: anchor offset + rule id + size. Same
        // (offset, rule, size) on retry produces the same key, so
        // re-drains after a crash dedupe via the UNIQUE constraint.
        // Anchor offset alone isn't enough — two rules could both
        // anchor on the same offset (rare; first-rule-wins handles
        // most cases, but the key is defensive).
        let synthetic_line = format!("{anchor_line}|burst:{}:{}", burst.rule_id, burst.size);
        let key = idempotency_key(log_source, anchor_offset, &synthetic_line);
        // Idempotency already covers retries; if the insert fails for
        // another reason, log and continue — better one missing summary
        // than no events at all.
        if let Err(e) = storage.insert_event(
            &key,
            &event_type,
            &ts,
            anchor_line,
            &payload,
            log_source,
            anchor_offset,
        ) {
            tracing::warn!(error = %e, rule = %burst.rule_id, "insert burst summary failed");
            continue;
        }
        let _ = end_buf_idx; // captured for future end-marker rendering
        let mut s = stats.lock();
        s.events_recognised += 1;
        s.last_event_type = Some(event_type);
        s.last_event_at = Some(ts);
    }

    // Pass 3: per-line ingest for everything not consumed by a burst.
    for (i, (line, line_offset)) in buffer.iter().enumerate() {
        if suppressed.contains(&i) {
            continue;
        }
        process_line(line, storage, stats, log_source, *line_offset, remote_rules);
    }
}

fn process_line(
    line: &str,
    storage: &Storage,
    stats: &parking_lot::Mutex<TailStats>,
    log_source: &str,
    line_offset: u64,
    rules: &[starstats_core::CompiledRemoteRule],
) {
    match ingest_one_line(line, storage, log_source, line_offset, rules) {
        IngestOutcome::Skipped => {
            stats.lock().lines_skipped += 1;
        }
        IngestOutcome::Noise => {
            stats.lock().lines_noise += 1;
        }
        IngestOutcome::StructuralOnly => {
            stats.lock().lines_structural_only += 1;
        }
        IngestOutcome::Recognised {
            event_type,
            timestamp,
        } => {
            let mut s = stats.lock();
            s.events_recognised += 1;
            s.last_event_type = Some(event_type);
            s.last_event_at = Some(timestamp);
        }
    }
}

/// What happened to a single line during ingest. Surfaced so the
/// caller can update its own stats — `process_line` (live tail) and
/// the backfill module both wrap this with their own counter shape.
#[derive(Debug)]
pub(crate) enum IngestOutcome {
    /// Structural parse failed — banner, blank, continuation line.
    Skipped,
    /// Structural parse OK; event_name was on the user's noise list.
    Noise,
    /// Structural parse OK; classifier didn't recognise the event_name
    /// (or it had no event_name). A sample is recorded in the
    /// `unknowns` table for surface area.
    StructuralOnly,
    /// Event classified, serialised, inserted (or deduped via the
    /// idempotency key — both paths return this).
    Recognised {
        event_type: String,
        timestamp: String,
    },
}

/// Stats-free ingest of one log line. The caller owns the stats
/// shape; this function only touches `storage`. Pulled out of
/// `process_line` so the backfill module can replay rotated
/// `Game-*.log` files into the same store without conflating its
/// counters with the live-tail counters.
pub(crate) fn ingest_one_line(
    line: &str,
    storage: &Storage,
    log_source: &str,
    line_offset: u64,
    remote_rules: &[starstats_core::CompiledRemoteRule],
) -> IngestOutcome {
    let Some(parsed) = structural_parse(line) else {
        return IngestOutcome::Skipped;
    };
    // Built-in classifier first; remote rules only run on built-in
    // miss so they can never override or suppress an authoritative
    // classification.
    let event = classify(&parsed).or_else(|| apply_remote_rules(&parsed, remote_rules));
    let Some(event) = event else {
        // Structural parse OK, classifier had no rule. Two paths:
        // 1. event_name is on the noise list → bump noise counter,
        //    don't pollute the actionable unknowns table.
        // 2. event_name is genuinely unknown → record a sample so the
        //    user can see what's missing a rule.
        // No event_name (rare — usually means the line is mid-flight
        // and the structural parser was over-permissive) → skip silently.
        if let Some(event_name) = parsed.event_name {
            match storage.is_noise(event_name) {
                Ok(true) => return IngestOutcome::Noise,
                Ok(false) => {
                    if let Err(e) =
                        storage.record_unknown(log_source, event_name, line, parsed.body)
                    {
                        tracing::warn!(error = %e, "record_unknown failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "is_noise query failed");
                }
            }
        }
        return IngestOutcome::StructuralOnly;
    };
    let Some((event_type, timestamp, payload)) = serialise_event(&event) else {
        return IngestOutcome::Skipped;
    };

    let idempotency_key = idempotency_key(log_source, line_offset, line);
    if let Err(e) = storage.insert_event(
        &idempotency_key,
        &event_type,
        &timestamp,
        line,
        &payload,
        log_source,
        line_offset,
    ) {
        tracing::warn!(error = %e, "insert_event failed");
        return IngestOutcome::Skipped;
    }

    IngestOutcome::Recognised {
        event_type,
        timestamp,
    }
}

/// Public wrapper around the private channel-derivation logic so the
/// backfill module can compute log_source from a rotated file's path.
pub(crate) fn log_source_from_path(path: &std::path::Path) -> String {
    // `Logs/Game-*.log` lives one level deeper than live `Game.log`,
    // so we look at the **grandparent** directory name when the
    // immediate parent is `Logs`. Otherwise the immediate parent.
    let parent = path.parent();
    let grandparent = parent.and_then(|p| p.parent());
    let segment = match parent.and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
        Some("Logs") => grandparent
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str()),
        Some(name) => Some(name),
        None => None,
    };
    let upper = segment.unwrap_or("OTHER").to_ascii_uppercase();
    match upper.as_str() {
        "LIVE" => "live",
        "PTU" => "ptu",
        "EPTU" => "eptu",
        "HOTFIX" => "hotfix",
        "TECH-PREVIEW" => "tech",
        _ => "other",
    }
    .to_string()
}

/// Stable per-line key. Same byte offset + same content always
/// produces the same key, so a re-tail of the same file (e.g. after
/// a crash recovery) hits the UNIQUE constraint instead of double-
/// inserting. UUIDv5 over the SHA-1 of (source || offset || line) —
/// 36 chars, deterministic, no clock dependency.
fn idempotency_key(log_source: &str, offset: u64, line: &str) -> String {
    use uuid::Uuid;
    let ns = Uuid::NAMESPACE_OID;
    let payload = format!("{log_source}:{offset}:{line}");
    Uuid::new_v5(&ns, payload.as_bytes()).to_string()
}

fn serialise_event(event: &GameEvent) -> Option<(String, String, String)> {
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
