import { useEffect, useState } from 'react';
import {
  api,
  type LogKind,
  type ParseCoverageResponse,
  type SourceStats,
  type StatusResponse,
  type TimelineEntry,
} from '../api';
import {
  Banner,
  GhostButton,
  KV,
  StatPill,
  StatusDot,
  TrayCard,
} from './tray/primitives';
import {
  ageLabel,
  fmtBytes,
  fmtCovPct,
  fmtTime,
  toneForType,
  TONE_VAR,
} from './tray/format';
import type { HangarStats } from '../api';

/// Only http(s) origins get rendered as a clickable link in the
/// email-verification banner. Defends against a hostile local config
/// that injects a `javascript:` / `data:` web_origin.
function isSafeWebOrigin(value: string | null): value is string {
  if (!value) return false;
  return /^https?:\/\//i.test(value);
}

/// Compact label + tone colour for each `LogKind`. Live = ok green
/// (it's the currently-tailed source); archived = info blue (passive
/// historical); crash = warn (forensic); launcher = neutral (not
/// gameplay).
const KIND_META: Record<LogKind, { label: string; tone: string }> = {
  channel_live: { label: 'LIVE', tone: 'var(--ok)' },
  channel_archived: { label: 'ARCH', tone: 'var(--info)' },
  crash_report: { label: 'CRASH', tone: 'var(--warn)' },
  launcher_log: { label: 'LNCH', tone: 'var(--fg-muted)' },
};

/// Display order for the "Discovered logs" kind breakdown chips.
/// Mirrors the declaration order of KIND_META so a freshly-added kind
/// also picks up a chip slot without code changes elsewhere — but
/// keeping the explicit array means the order is intentional, not
/// accidentally object-key-iteration-dependent.
const KIND_ORDER: LogKind[] = [
  'channel_live',
  'channel_archived',
  'crash_report',
  'launcher_log',
];

/// Group discovered logs by `kind` and return one row per non-empty
/// bucket. Used by the Discovered logs card to render a compact
/// breakdown chip row instead of a per-file list.
function kindBreakdown(
  logs: ReadonlyArray<{ kind: LogKind }>
): Array<{ kind: LogKind; count: number }> {
  const counts = new Map<LogKind, number>();
  for (const log of logs) {
    counts.set(log.kind, (counts.get(log.kind) ?? 0) + 1);
  }
  return KIND_ORDER.flatMap((kind) => {
    const count = counts.get(kind);
    return count ? [{ kind, count }] : [];
  });
}

interface Props {
  status: StatusResponse;
  /// Web UI origin used by the email-verification banner link.
  /// `null` if the user hasn't configured an API URL yet — we hide
  /// the link in that case so we don't render a dangling anchor.
  webOrigin: string | null;
  /// Routes the user to the Settings pane where the pairing form
  /// lives. Used by the auth-lost banner's CTA.
  onGoToSettings: () => void;
}

export function StatusPane({ status, webOrigin, onGoToSettings }: Props) {
  const {
    tail,
    sync,
    event_counts,
    total_events,
    discovered_logs,
    account,
    hangar,
  } = status;
  const [coverage, setCoverage] = useState<ParseCoverageResponse | null>(
    null,
  );
  const [timeline, setTimeline] = useState<TimelineEntry[] | null>(null);
  const [sourceStats, setSourceStats] = useState<SourceStats | null>(null);
  // Per-row mark-as-noise error message keyed by event_name. A user
  // who clicks "ignore" expects the row to disappear; if the mutation
  // fails we have to say so, otherwise they'll click again and again
  // while the row stays put.
  const [noiseErrors, setNoiseErrors] = useState<Record<string, string>>({});
  // Set of event_names with an in-flight mark-as-noise call, to
  // disable double-clicks and surface a "working…" affordance.
  const [pendingNoise, setPendingNoise] = useState<Set<string>>(new Set());

  useEffect(() => {
    let cancelled = false;

    const fetchAll = async () => {
      try {
        const [cov, tl, ss] = await Promise.all([
          api.getParseCoverage(),
          api.getSessionTimeline(),
          api.getSourceStats(),
        ]);
        if (!cancelled) {
          setCoverage(cov);
          setTimeline(tl);
          setSourceStats(ss);
        }
      } catch (err) {
        // Keep UI quiet on failure; surface to dev console only.
        // eslint-disable-next-line no-console
        console.warn('Failed to refresh status data', err);
      }
    };

    fetchAll();
    const handle = window.setInterval(fetchAll, 15_000);

    return () => {
      cancelled = true;
      window.clearInterval(handle);
    };
  }, []);

  const handleMarkAsNoise = async (eventName: string) => {
    setPendingNoise((prev) => {
      const next = new Set(prev);
      next.add(eventName);
      return next;
    });
    setNoiseErrors((prev) => {
      if (!(eventName in prev)) return prev;
      const { [eventName]: _removed, ...rest } = prev;
      return rest;
    });
    try {
      await api.markEventAsNoise(eventName);
      const refreshed = await api.getParseCoverage();
      setCoverage(refreshed);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'unknown error';
      setNoiseErrors((prev) => ({ ...prev, [eventName]: message }));
    } finally {
      setPendingNoise((prev) => {
        const next = new Set(prev);
        next.delete(eventName);
        return next;
      });
    }
  };

  const hasSyncActivity =
    sync.last_attempt_at !== null || sync.batches_sent > 0;

  // Banner precedence: auth_lost > email_verified === false. If
  // we're not paired the email-verify nag is pointless — fixing
  // pairing is strictly the higher-priority action.
  const showAuthLost = account.auth_lost;
  const showEmailUnverified =
    !showAuthLost && account.email_verified === false;

  // Top-types ranked bar — denominator clamped to 1 to avoid
  // divide-by-zero when event_counts is empty (handled separately
  // below, but still defensive).
  const maxCount = Math.max(...event_counts.map((c) => c.count), 1);

  const totalSyncEvents =
    sync.events_accepted + sync.events_duplicate + sync.events_rejected;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {showAuthLost && (
        <Banner tone="warn" action="Re-pair" onAction={onGoToSettings}>
          This device is no longer paired with your account.
        </Banner>
      )}
      {showEmailUnverified && (
        <Banner tone="info">
          Your Comm-Link isn&apos;t verified.{' '}
          {isSafeWebOrigin(webOrigin) ? (
            <a
              href={webOrigin}
              target="_blank"
              rel="noreferrer"
              style={{
                color: 'inherit',
                textDecoration: 'underline',
                fontWeight: 600,
              }}
            >
              Verify it on the web
            </a>
          ) : (
            'Verify it on the web'
          )}
          .
        </Banner>
      )}

      {/* HEADLINE STAT STRIP */}
      <div style={{ display: 'flex', gap: 8 }}>
        <StatPill
          label="Events"
          value={total_events.toLocaleString()}
          tone="accent"
        />
        <StatPill
          label="Lines"
          value={tail.lines_processed.toLocaleString()}
        />
        <StatPill
          label="Batches"
          value={sync.batches_sent.toLocaleString()}
        />
        <StatPill
          label="Coverage"
          value={
            coverage
              ? fmtCovPct(coverage.recognised, coverage.structural_only)
              : '—'
          }
          tone="ok"
        />
      </div>

      {/* TAILING + SYNC, side-by-side */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: '1fr 1fr',
          gap: 12,
        }}
      >
        <TrayCard
          title="Tailing"
          kicker={tail.current_path ? 'LIVE' : 'IDLE'}
        >
          {tail.current_path ? (
            <dl
              style={{
                display: 'grid',
                gridTemplateColumns: '90px 1fr',
                gap: '6px 10px',
                margin: 0,
              }}
            >
              <KV label="Path" value={tail.current_path} mono />
              <KV label="Read" value={fmtBytes(tail.bytes_read)} />
              <KV
                label="Last event"
                value={
                  tail.last_event_type ? (
                    <>
                      <code
                        style={{ color: 'var(--accent)', fontSize: 12 }}
                      >
                        {tail.last_event_type}
                      </code>{' '}
                      <span style={{ color: 'var(--fg-dim)' }}>
                        · {fmtTime(tail.last_event_at)}
                      </span>
                    </>
                  ) : (
                    <span style={{ color: 'var(--fg-dim)' }}>—</span>
                  )
                }
              />
            </dl>
          ) : (
            <p
              style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}
            >
              Scope is clear. No Game.log discovered yet — set a custom
              path in Settings.
            </p>
          )}
        </TrayCard>

        <TrayCard
          title="Remote sync"
          right={
            <span
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                fontSize: 11,
                color: sync.last_error ? 'var(--danger)' : 'var(--ok)',
              }}
            >
              <StatusDot tone={sync.last_error ? 'danger' : 'ok'} />
              <span
                style={{
                  fontFamily: 'var(--font-mono)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                }}
              >
                {sync.last_error ? 'ERR' : 'OK'}
              </span>
            </span>
          }
        >
          {hasSyncActivity ? (
            <dl
              style={{
                display: 'grid',
                gridTemplateColumns: '90px 1fr',
                gap: '6px 10px',
                margin: 0,
              }}
            >
              <KV
                label="Last sync"
                value={
                  sync.last_success_at ? fmtTime(sync.last_success_at) : '—'
                }
                mono
              />
              <KV
                label="Accepted"
                value={`${sync.events_accepted.toLocaleString()} / ${totalSyncEvents.toLocaleString()}`}
                mono
              />
              <KV
                label="Dup / rej"
                value={`${sync.events_duplicate} · ${sync.events_rejected}`}
                mono
                dim
              />
            </dl>
          ) : (
            <p
              style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}
            >
              Disabled. Configure remote sync in Settings to push events to
              an API server.
            </p>
          )}
        </TrayCard>
      </div>

      {/* TOP TYPES */}
      {event_counts.length === 0 ? (
        <TrayCard title="Top event types">
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            No events captured yet.
          </p>
        </TrayCard>
      ) : (
        <TrayCard
          title="Top event types"
          kicker={`${event_counts.length} types`}
        >
          <div
            style={{ display: 'flex', flexDirection: 'column', gap: 5 }}
          >
            {event_counts.map((c) => {
              const pct = (c.count / maxCount) * 100;
              return (
                <div
                  key={c.event_type}
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '1fr auto',
                    gap: 8,
                    alignItems: 'center',
                    position: 'relative',
                  }}
                >
                  <div style={{ position: 'relative' }}>
                    <div
                      style={{
                        position: 'absolute',
                        inset: 0,
                        background: 'var(--surface-2)',
                        borderRadius: 'var(--r-xs)',
                      }}
                    />
                    <div
                      style={{
                        position: 'absolute',
                        top: 0,
                        bottom: 0,
                        left: 0,
                        width: `${pct}%`,
                        background:
                          'linear-gradient(90deg, var(--accent-soft) 0%, transparent 100%)',
                        borderRadius: 'var(--r-xs)',
                        borderLeft: '2px solid var(--accent)',
                      }}
                    />
                    <code
                      style={{
                        position: 'relative',
                        display: 'block',
                        padding: '4px 8px',
                        fontSize: 11.5,
                        color: 'var(--fg)',
                        fontFamily: 'var(--font-mono)',
                      }}
                    >
                      {c.event_type}
                    </code>
                  </div>
                  <span
                    style={{
                      fontFamily: 'var(--font-mono)',
                      fontSize: 12,
                      color: 'var(--fg-muted)',
                      fontVariantNumeric: 'tabular-nums',
                      minWidth: 44,
                      textAlign: 'right',
                    }}
                  >
                    {c.count.toLocaleString()}
                  </span>
                </div>
              );
            })}
          </div>
        </TrayCard>
      )}

      {/* TIMELINE */}
      <TrayCard
        title="Session timeline"
        kicker={`${timeline?.length ?? 0} entries`}
      >
        {timeline === null ? (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            Loading timeline…
          </p>
        ) : timeline.length === 0 ? (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            Scope is clear. Launch Star Citizen to start the feed.
          </p>
        ) : (
          <ol
            style={{
              listStyle: 'none',
              margin: 0,
              padding: 0,
              display: 'flex',
              flexDirection: 'column',
              gap: 2,
              maxHeight: 300,
              overflowY: 'auto',
            }}
          >
            {timeline.map((e) => {
              const accent = TONE_VAR[toneForType(e.event_type)];
              return (
                <li
                  key={e.id}
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '60px 160px 1fr',
                    gap: 10,
                    alignItems: 'baseline',
                    padding: '4px 6px',
                    borderLeft: `2px solid ${accent}`,
                    fontSize: 12,
                  }}
                >
                  <span
                    style={{
                      color: 'var(--fg-dim)',
                      fontFamily: 'var(--font-mono)',
                    }}
                    title={e.timestamp}
                  >
                    {fmtTime(e.timestamp)}
                  </span>
                  <code
                    style={{
                      color: accent,
                      fontSize: 11,
                      textTransform: 'uppercase',
                      letterSpacing: '0.04em',
                      fontFamily: 'var(--font-mono)',
                      whiteSpace: 'nowrap',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                    }}
                  >
                    {e.event_type}
                  </code>
                  <span style={{ color: 'var(--fg)' }}>{e.summary}</span>
                </li>
              );
            })}
          </ol>
        )}
      </TrayCard>

      {/* COVERAGE */}
      <TrayCard title="Parser coverage">
        {coverage === null ? (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            Loading parser coverage…
          </p>
        ) : (
          <>
            <div
              style={{
                display: 'flex',
                alignItems: 'baseline',
                gap: 12,
                marginBottom: 6,
              }}
            >
              <span
                style={{
                  fontFamily: 'var(--font-mono)',
                  fontSize: 28,
                  fontWeight: 700,
                  color: 'var(--accent)',
                  fontVariantNumeric: 'tabular-nums',
                  lineHeight: 1,
                }}
              >
                {fmtCovPct(coverage.recognised, coverage.structural_only)}
              </span>
              <span style={{ fontSize: 12, color: 'var(--fg-muted)' }}>
                recognised lines
              </span>
            </div>
            <div
              style={{
                fontSize: 11,
                color: 'var(--fg-dim)',
                fontFamily: 'var(--font-mono)',
              }}
            >
              {coverage.recognised.toLocaleString()} ok ·{' '}
              {coverage.structural_only.toLocaleString()} unknown ·{' '}
              {coverage.noise.toLocaleString()} noise ·{' '}
              {coverage.skipped.toLocaleString()} skipped
            </div>

            {coverage.unknowns.length === 0 ? (
              <p
                style={{
                  margin: '10px 0 0',
                  color: 'var(--fg-dim)',
                  fontSize: 13,
                }}
              >
                Nothing unrecognized yet — your parser rules cover
                everything we&apos;ve seen.
              </p>
            ) : (
              <details style={{ marginTop: 10 }}>
                <summary
                  style={{
                    cursor: 'pointer',
                    color: 'var(--fg-muted)',
                    fontSize: 12,
                    padding: '4px 0',
                  }}
                >
                  Unrecognised types ({coverage.unknowns.length})
                </summary>
                <ul
                  style={{
                    listStyle: 'none',
                    padding: 0,
                    margin: '8px 0 0',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 6,
                  }}
                >
                  {coverage.unknowns.map((u) => (
                    <li
                      key={`${u.log_source}:${u.event_name}`}
                      style={{
                        border: '1px solid var(--border)',
                        borderRadius: 'var(--r-sm)',
                        padding: '6px 8px',
                        background: 'var(--bg)',
                      }}
                    >
                      <div
                        style={{
                          display: 'grid',
                          gridTemplateColumns: 'auto 1fr auto auto',
                          gap: 8,
                          alignItems: 'center',
                          fontSize: 11.5,
                        }}
                      >
                        <span
                          style={{
                            background: 'var(--accent-soft)',
                            color: 'var(--accent)',
                            fontSize: 10,
                            fontWeight: 700,
                            padding: '1px 6px',
                            borderRadius: 'var(--r-xs)',
                            letterSpacing: '0.04em',
                          }}
                        >
                          {u.log_source}
                        </span>
                        <code
                          style={{
                            color: 'var(--accent)',
                            fontFamily: 'var(--font-mono)',
                            wordBreak: 'break-all',
                          }}
                        >
                          {u.event_name}
                        </code>
                        <span
                          style={{
                            color: 'var(--fg-muted)',
                            fontFamily: 'var(--font-mono)',
                            fontVariantNumeric: 'tabular-nums',
                          }}
                        >
                          {u.occurrences.toLocaleString()}
                        </span>
                        <button
                          type="button"
                          disabled={pendingNoise.has(u.event_name)}
                          onClick={() => handleMarkAsNoise(u.event_name)}
                          title="Add to noise list — drops the existing sample and stops recording new ones"
                          style={{
                            background: 'transparent',
                            border: '1px solid var(--border-strong)',
                            color: 'var(--fg-muted)',
                            borderRadius: 'var(--r-xs)',
                            padding: '1px 6px',
                            fontSize: 10,
                            cursor: pendingNoise.has(u.event_name)
                              ? 'not-allowed'
                              : 'pointer',
                            opacity: pendingNoise.has(u.event_name)
                              ? 0.55
                              : 1,
                            fontFamily: 'inherit',
                          }}
                        >
                          {pendingNoise.has(u.event_name)
                            ? 'working…'
                            : 'ignore'}
                        </button>
                      </div>
                      {noiseErrors[u.event_name] && (
                        <p
                          role="alert"
                          style={{
                            margin: '4px 0 0',
                            fontSize: 11,
                            color: 'var(--danger)',
                          }}
                        >
                          Couldn&apos;t mark as noise:{' '}
                          {noiseErrors[u.event_name]}
                        </p>
                      )}
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </>
        )}
      </TrayCard>

      {/* DISCOVERED LOGS */}
      <TrayCard
        title="Discovered logs"
        kicker={`${discovered_logs.length} found`}
      >
        {discovered_logs.length === 0 ? (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            No Game.log files discovered.
          </p>
        ) : (
          <>
            <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
              All {discovered_logs.length} discovered{' '}
              {discovered_logs.length === 1 ? 'log is' : 'logs are'} being read;
              events from each are included in the tray pipeline.
            </p>
            <div
              style={{
                display: 'flex',
                flexWrap: 'wrap',
                gap: 6,
                marginTop: 8,
              }}
            >
              {kindBreakdown(discovered_logs).map(({ kind, count }) => {
                const meta = KIND_META[kind];
                return (
                  <span
                    key={kind}
                    title={kind}
                    style={{
                      background: meta.tone,
                      color: 'var(--bg)',
                      fontSize: 10,
                      fontWeight: 700,
                      padding: '2px 6px',
                      borderRadius: 'var(--r-xs)',
                      letterSpacing: '0.06em',
                    }}
                  >
                    {count} {meta.label}
                  </span>
                );
              })}
            </div>
          </>
        )}
      </TrayCard>

      {/* SOURCES */}
      <SourcesCard stats={sourceStats} />

      {/* HANGAR */}
      <HangarCard hangar={hangar} />
    </div>
  );
}

/// "Sources" card — surfaces the secondary-pipeline stats (launcher
/// tail, crash scanner, rotated-log backfill) that the tray now runs
/// alongside the live Game.log tail. None of these are blocking; the
/// card just confirms each pipeline is doing something so the user
/// can spot a stuck one.
function SourcesCard({ stats }: { stats: SourceStats | null }) {
  if (!stats) {
    return (
      <TrayCard title="Sources" kicker="loading…">
        <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
          Reading pipeline stats…
        </p>
      </TrayCard>
    );
  }

  const { launcher, crashes, backfill } = stats;
  const backfillKicker = backfill.completed
    ? `${backfill.files_processed + backfill.files_already_done}/${backfill.files_total} files`
    : `scanning… ${backfill.files_processed}/${backfill.files_total}`;

  return (
    <TrayCard title="Sources" kicker={backfillKicker}>
      <dl
        style={{
          display: 'grid',
          gridTemplateColumns: '110px 1fr',
          gap: '6px 10px',
          margin: 0,
        }}
      >
        <KV
          label="Launcher"
          value={
            launcher.current_path === null ? (
              <span style={{ color: 'var(--fg-dim)' }}>not running</span>
            ) : (
              <span>
                {launcher.events_recognised.toLocaleString()} events
                {launcher.last_category && (
                  <>
                    {' · last: '}
                    <span
                      style={{
                        textTransform: 'uppercase',
                        fontSize: 10,
                        fontWeight: 700,
                        letterSpacing: '0.06em',
                        color:
                          launcher.last_category === 'error'
                            ? 'var(--err)'
                            : 'var(--accent)',
                      }}
                    >
                      {launcher.last_category}
                    </span>
                  </>
                )}
              </span>
            )
          }
        />
        <KV
          label="Crashes"
          value={
            crashes.total_crashes_seen === 0 ? (
              <span style={{ color: 'var(--ok)' }}>none on disk</span>
            ) : (
              <span>
                {crashes.total_crashes_seen} on disk
                {crashes.last_crash_dir && (
                  <span
                    className="mono"
                    style={{
                      color: 'var(--fg-dim)',
                      fontSize: 11,
                      marginLeft: 6,
                    }}
                  >
                    last: {crashes.last_crash_dir}
                  </span>
                )}
              </span>
            )
          }
        />
        <KV
          label="Backfill"
          value={
            backfill.completed ? (
              <span>
                {backfill.events_recognised.toLocaleString()} events from{' '}
                {backfill.files_processed} archived
                {backfill.files_already_done > 0 && (
                  <span style={{ color: 'var(--fg-dim)' }}>
                    {' '}
                    ({backfill.files_already_done} already done)
                  </span>
                )}
              </span>
            ) : (
              <span style={{ color: 'var(--info)' }}>
                replaying historical sessions…
              </span>
            )
          }
        />
      </dl>
    </TrayCard>
  );
}

/// Render-state derived from `HangarStats`. Field combinations encode
/// six distinct UX states; the previous "ERROR else OK" kicker treated
/// "never ran" identically to "succeeded", which is what produced the
/// misleading "OK · no last sync" rendering. We always look at all
/// three timestamps (attempt/success/error) plus the skip reason.
type HangarState =
  | { kind: 'never_started' }
  | { kind: 'refreshing' }
  | { kind: 'fresh_success'; at: string; ageMs: number }
  | { kind: 'stale_success'; at: string }
  | { kind: 'skipped'; at: string | null; reason: string }
  | { kind: 'error'; at: string | null; message: string };

/// Anything fresher than this counts as a "just-fetched" affirmative
/// success (green dot). Past the window we still call it a success but
/// fade it to the muted "info" tone.
const HANGAR_FRESH_MS = 5 * 60_000;

/// Hard ceiling on the in-flight spinner. If the polled
/// `last_attempt_at` hasn't advanced past the click stamp by then,
/// we drop back to the previous state rather than spin forever — the
/// kick worker may not even be spawned (no api_url/token configured).
const HANGAR_REFRESH_TIMEOUT_MS = 60_000;

function deriveHangarState(
  h: HangarStats,
  refreshingSince: number | null,
): HangarState {
  if (refreshingSince !== null) {
    return { kind: 'refreshing' };
  }
  if (h.last_error) {
    return {
      kind: 'error',
      at: h.last_attempt_at,
      message: h.last_error,
    };
  }
  if (h.last_success_at) {
    const ageMs = Date.now() - new Date(h.last_success_at).getTime();
    if (ageMs < HANGAR_FRESH_MS) {
      return { kind: 'fresh_success', at: h.last_success_at, ageMs };
    }
    return { kind: 'stale_success', at: h.last_success_at };
  }
  if (h.last_skip_reason) {
    return {
      kind: 'skipped',
      at: h.last_attempt_at,
      reason: h.last_skip_reason,
    };
  }
  return { kind: 'never_started' };
}

interface HangarCardProps {
  hangar: HangarStats;
}

function HangarCard({ hangar }: HangarCardProps) {
  // Stamped on click; cleared once a polled `last_attempt_at` lands
  // past the stamp (see effect below). Without this, "Refresh now"
  // would leave no trace in the UI between clicks until the next
  // poll arrives.
  const [refreshingSince, setRefreshingSince] = useState<number | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);

  useEffect(() => {
    if (refreshingSince === null) return;

    const lastAttemptMs = hangar.last_attempt_at
      ? new Date(hangar.last_attempt_at).getTime()
      : 0;
    if (lastAttemptMs >= refreshingSince) {
      setRefreshingSince(null);
      return;
    }

    // Safety net: if no new attempt lands within the timeout, give
    // up so the spinner doesn't stick. The hangar worker only runs
    // when api_url + access_token are both set; the kick is a no-op
    // otherwise.
    const remaining =
      refreshingSince + HANGAR_REFRESH_TIMEOUT_MS - Date.now();
    const handle = window.setTimeout(
      () => setRefreshingSince(null),
      Math.max(0, remaining),
    );
    return () => window.clearTimeout(handle);
  }, [hangar.last_attempt_at, refreshingSince]);

  const state = deriveHangarState(hangar, refreshingSince);
  const dotTone = hangarDotTone(state);
  const kicker = hangarKicker(state);

  const onRefresh = async () => {
    setRefreshError(null);
    setRefreshingSince(Date.now());
    try {
      await api.refreshHangarNow();
    } catch (err) {
      setRefreshingSince(null);
      setRefreshError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <TrayCard
      title="Hangar"
      kicker={
        <span
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
          }}
        >
          <StatusDot tone={dotTone} />
          {kicker}
        </span>
      }
      right={
        <GhostButton
          onClick={onRefresh}
          disabled={state.kind === 'refreshing'}
          style={{ padding: '4px 10px', fontSize: 11 }}
        >
          {state.kind === 'refreshing' ? 'Refreshing…' : 'Refresh now'}
        </GhostButton>
      }
    >
      <HangarBody state={state} ships={hangar.ships_pushed} />
      {refreshError && (
        <p
          style={{
            margin: '8px 0 0',
            fontSize: 12,
            color: 'var(--danger)',
          }}
        >
          Refresh failed: {refreshError}
        </p>
      )}
    </TrayCard>
  );
}

function hangarDotTone(state: HangarState): 'ok' | 'warn' | 'danger' | 'info' | 'dim' {
  switch (state.kind) {
    case 'fresh_success':
      return 'ok';
    case 'stale_success':
      return 'info';
    case 'refreshing':
      return 'info';
    case 'skipped':
      return 'warn';
    case 'error':
      return 'danger';
    case 'never_started':
      return 'dim';
  }
}

function hangarKicker(state: HangarState): string {
  switch (state.kind) {
    case 'never_started':
      return 'not started';
    case 'refreshing':
      return 'fetching from RSI…';
    case 'fresh_success':
      return `✓ ${ageLabel(state.at)}`;
    case 'stale_success':
      return ageLabel(state.at);
    case 'skipped':
      return 'skipped';
    case 'error':
      return 'error';
  }
}

function HangarBody({ state, ships }: { state: HangarState; ships: number }) {
  const shipCount = ships.toLocaleString();

  switch (state.kind) {
    case 'never_started':
      return (
        <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
          Pair this device and configure the API URL to start syncing your
          hangar from the RSI website.
        </p>
      );

    case 'refreshing':
      return (
        <p style={{ margin: 0, color: 'var(--fg)', fontSize: 13 }}>
          Fetching the latest hangar snapshot from RSI… last known total{' '}
          <strong style={{ fontVariantNumeric: 'tabular-nums' }}>
            {shipCount}
          </strong>{' '}
          ships.
        </p>
      );

    case 'fresh_success':
      return (
        <p style={{ margin: 0, color: 'var(--fg)', fontSize: 13 }}>
          <span style={{ color: 'var(--ok)' }}>
            Fetched{' '}
            <strong style={{ fontVariantNumeric: 'tabular-nums' }}>
              {shipCount}
            </strong>{' '}
            ships from RSI
          </span>{' '}
          <span style={{ color: 'var(--fg-dim)' }}>
            · {ageLabel(state.at)} ({fmtTime(state.at)})
          </span>
        </p>
      );

    case 'stale_success':
      return (
        <p style={{ margin: 0, color: 'var(--fg)', fontSize: 13 }}>
          Last successful fetch{' '}
          <strong>{ageLabel(state.at)}</strong>{' '}
          <span style={{ color: 'var(--fg-dim)' }}>
            ({fmtTime(state.at)})
          </span>{' '}
          ·{' '}
          <strong style={{ fontVariantNumeric: 'tabular-nums' }}>
            {shipCount}
          </strong>{' '}
          ships pushed
        </p>
      );

    case 'skipped':
      return (
        <p style={{ margin: 0, color: 'var(--warn)', fontSize: 13 }}>
          Skipped: {state.reason}
          {state.at && (
            <span style={{ color: 'var(--fg-dim)' }}>
              {' '}
              · {ageLabel(state.at)}
            </span>
          )}
        </p>
      );

    case 'error':
      return (
        <p style={{ margin: 0, color: 'var(--danger)', fontSize: 13 }}>
          {state.message}
          {state.at && (
            <span style={{ color: 'var(--fg-dim)' }}>
              {' '}
              · {ageLabel(state.at)}
            </span>
          )}
        </p>
      );
  }
}
