import { useEffect, useState } from 'react';
import {
  api,
  type LogKind,
  type ParseCoverageResponse,
  type SourceStats,
  type StatusResponse,
  type TimelineEntry,
} from '../api';
import { Banner, KV, StatPill, StatusDot, TrayCard } from './tray/primitives';
import {
  fmtBytes,
  fmtCovPct,
  fmtTime,
  toneForType,
  TONE_VAR,
} from './tray/format';

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
          <ul
            style={{
              listStyle: 'none',
              margin: 0,
              padding: 0,
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
            }}
          >
            {discovered_logs.map((l) => {
              const meta = KIND_META[l.kind];
              return (
                <li
                  key={l.path}
                  style={{
                    display: 'grid',
                    gridTemplateColumns: 'auto auto 1fr auto',
                    gap: 8,
                    alignItems: 'center',
                    fontSize: 12,
                    padding: '4px 0',
                  }}
                >
                  <span
                    style={{
                      background:
                        l.channel === 'LIVE' ? 'var(--ok)' : 'var(--info)',
                      color: 'var(--bg)',
                      fontSize: 10,
                      fontWeight: 700,
                      padding: '1px 6px',
                      borderRadius: 'var(--r-xs)',
                      letterSpacing: '0.06em',
                    }}
                  >
                    {l.channel}
                  </span>
                  <span
                    title={l.kind}
                    style={{
                      background: meta.tone,
                      color: 'var(--bg)',
                      fontSize: 9,
                      fontWeight: 700,
                      padding: '1px 5px',
                      borderRadius: 'var(--r-xs)',
                      letterSpacing: '0.06em',
                    }}
                  >
                    {meta.label}
                  </span>
                  <code
                    style={{
                      color: 'var(--fg)',
                      fontFamily: 'var(--font-mono)',
                      fontSize: 11,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                    title={l.path}
                  >
                    {l.path}
                  </code>
                  <span
                    style={{
                      color: 'var(--fg-muted)',
                      fontFamily: 'var(--font-mono)',
                      fontSize: 11,
                    }}
                  >
                    {fmtBytes(l.size_bytes)}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </TrayCard>

      {/* SOURCES */}
      <SourcesCard stats={sourceStats} />

      {/* HANGAR */}
      <TrayCard
        title="Hangar"
        kicker={hangar.last_error ? 'ERROR' : 'OK'}
      >
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
              hangar.last_success_at ? fmtTime(hangar.last_success_at) : '—'
            }
            mono
          />
          <KV
            label="Ships pushed"
            value={hangar.ships_pushed.toLocaleString()}
            mono
          />
          <KV
            label="Status"
            value={
              hangar.last_skip_reason ??
              hangar.last_error ?? (
                <span style={{ color: 'var(--ok)' }}>ok</span>
              )
            }
          />
        </dl>
      </TrayCard>
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
