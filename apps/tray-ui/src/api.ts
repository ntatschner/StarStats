import { invoke } from '@tauri-apps/api/core';

/**
 * What kind of artifact a `DiscoveredLog` points at. Matches the Rust
 * `LogKind` enum (snake_case-serialised). Today only `channel_live`
 * is actually tailed by the client; the rest are surfaced for visibility
 * and as a seed for future ingest paths.
 */
export type LogKind =
  | 'channel_live'
  | 'channel_archived'
  | 'crash_report'
  | 'launcher_log';

export interface DiscoveredLog {
  channel: string;
  kind: LogKind;
  path: string;
  size_bytes: number;
}

export interface TailStats {
  current_path: string | null;
  bytes_read: number;
  lines_processed: number;
  events_recognised: number;
  last_event_at: string | null;
  last_event_type: string | null;
  lines_structural_only: number;
  lines_skipped: number;
  lines_noise: number;
}

export interface EventCount {
  event_type: string;
  count: number;
}

export interface SyncStats {
  last_attempt_at: string | null;
  last_success_at: string | null;
  last_error: string | null;
  batches_sent: number;
  events_accepted: number;
  events_duplicate: number;
  events_rejected: number;
}

export interface AccountStatus {
  /// True once the API has rejected the device token (401/403).
  /// Cleared by a successful re-pair.
  auth_lost: boolean;
  /// Mirror of `MeResponse.email_verified`. `null` until the first
  /// successful `GET /v1/auth/me` call lands.
  email_verified: boolean | null;
}

export interface HangarStats {
  last_attempt_at: string | null;
  last_success_at: string | null;
  last_error: string | null;
  ships_pushed: number;
  last_skip_reason: string | null;
}

export interface RsiCookieStatus {
  configured: boolean;
  preview: string | null;
}

export interface StatusResponse {
  tail: TailStats;
  sync: SyncStats;
  event_counts: EventCount[];
  total_events: number;
  discovered_logs: DiscoveredLog[];
  account: AccountStatus;
  hangar: HangarStats;
}

export interface RemoteSyncConfig {
  enabled: boolean;
  api_url: string | null;
  claimed_handle: string | null;
  access_token: string | null;
  interval_secs: number;
  batch_size: number;
}

export interface Config {
  gamelog_path: string | null;
  remote_sync: RemoteSyncConfig;
  web_origin: string | null;
  /// Whether to automatically check for updates on app startup.
  /// Defaults to true server-side; the Updates card in Settings
  /// exposes a toggle.
  auto_update_check: boolean;
  /// Whether to write a daily-rolling `client.log` to the user
  /// data dir. Defaults to false so end users have no log clutter;
  /// toggle on from Settings → Updates to capture logs for a bug
  /// report. The panic-only log is always written.
  debug_logging: boolean;
}

export interface PairOutcome {
  claimed_handle: string;
  label: string;
}

export interface UnknownSample {
  log_source: string;
  event_name: string;
  occurrences: number;
  first_seen: string;
  last_seen: string;
  sample_line: string;
  sample_body: string;
}

export interface ParseCoverageResponse {
  recognised: number;
  structural_only: number;
  skipped: number;
  noise: number;
  unknowns: UnknownSample[];
}

/** Snapshot of the launcher-log tailer. `current_path` is null when
 * no launcher logs were discovered locally. */
export interface LauncherStats {
  current_path: string | null;
  bytes_read: number;
  lines_processed: number;
  events_recognised: number;
  last_event_at: string | null;
  last_level: string | null;
  last_category: string | null;
  lines_skipped: number;
}

/** Snapshot of the crash-dir scanner. `last_crash_dir` is the most
 * recent crash on disk (newest-first by dir name). */
export interface CrashStats {
  last_scan_at: string | null;
  total_crashes_seen: number;
  last_crash_dir: string | null;
}

/** Snapshot of the rotated-log backfill task. `completed = true` means
 * the initial sweep finished; `false` means it's still scanning. */
export interface BackfillStats {
  completed: boolean;
  files_total: number;
  files_processed: number;
  files_already_done: number;
  lines_processed: number;
  events_recognised: number;
}

export interface SourceStats {
  launcher: LauncherStats;
  crashes: CrashStats;
  backfill: BackfillStats;
}

export interface TimelineEntry {
  id: number;
  timestamp: string;
  event_type: string;
  summary: string;
  raw_line: string;
  /// Channel tag (LIVE/PTU/EPTU) the event was tailed from.
  log_source: string;
  synced: boolean;
}

export interface StorageStats {
  total_events: number;
  db_size_bytes: number;
}

export type TransactionKind = 'shop' | 'commodity_buy' | 'commodity_sell';
export type TransactionStatus =
  | 'pending'
  | 'confirmed'
  | 'rejected'
  | 'timed_out'
  | 'submitted';

export interface Transaction {
  kind: TransactionKind;
  status: TransactionStatus;
  started_at: string;
  confirmed_at: string | null;
  shop_id: string | null;
  item: string | null;
  quantity: number | null;
  raw_request: string;
  raw_response: string | null;
}

export const api = {
  getStatus: () => invoke<StatusResponse>('get_status'),
  getConfig: () => invoke<Config>('get_config'),
  saveConfig: (cfg: Config) => invoke<void>('save_config', { cfg }),
  getDiscoveredLogs: () => invoke<DiscoveredLog[]>('get_discovered_logs'),
  pairDevice: (apiUrl: string, code: string) =>
    invoke<PairOutcome>('pair_device', { apiUrl, code }),
  getParseCoverage: () =>
    invoke<ParseCoverageResponse>('get_parse_coverage'),
  getSessionTimeline: (limit?: number) =>
    invoke<TimelineEntry[]>('get_session_timeline', { limit }),
  listTransactions: (limit?: number, windowSecs?: number) =>
    invoke<Transaction[]>('list_transactions', {
      limit,
      windowSecs,
    }),
  getSourceStats: () => invoke<SourceStats>('get_source_stats'),
  getStorageStats: () => invoke<StorageStats>('get_storage_stats'),
  /** Cargo workspace version (e.g. "0.2.0-alpha") — matches the
   * GitHub release tag. Distinct from Tauri's getVersion() which
   * returns the numeric tauri.conf.json version (MSI-friendly). */
  getAppVersion: () => invoke<string>('get_app_version'),
  markEventAsNoise: (eventName: string) =>
    invoke<void>('mark_event_as_noise', { eventName }),
  refreshAccountInfo: () => invoke<AccountStatus>('refresh_account_info'),
  retrySyncNow: () => invoke<void>('retry_sync_now'),
  setRsiCookie: (cookieValue: string) =>
    invoke<RsiCookieStatus>('set_rsi_cookie', { req: { cookieValue } }),
  clearRsiCookie: () => invoke<RsiCookieStatus>('clear_rsi_cookie'),
  getRsiCookieStatus: () => invoke<RsiCookieStatus>('get_rsi_cookie_status'),
};
