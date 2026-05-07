/**
 * Tray UI — Logs view: browse the locally-stored events on disk.
 *
 * Visual reference: `design/tray-prototype/tray-logs.jsx`. Where the
 * mock generated 140 fake events client-side, this component pulls
 * the live `recent_events` slice via `api.getSessionTimeline(1000)`
 * and the headline counters via `api.getStorageStats()`. Both refetch
 * on a 10s tick while mounted.
 *
 * Filtering, grouping, and the detail drawer are all client-side. We
 * pull a deeper window (1000 rows) than StatusPane's glance view
 * because the user expects to browse a session's worth of events
 * here, not just the last few. Server-side pagination is the next
 * step if anyone routinely sits on more than 1000 rows.
 */

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
} from 'react';
import {
  api,
  type StorageStats,
  type TimelineEntry,
} from '../api';
import {
  GhostButton,
  StatPill,
  StatusDot,
  TrayCard,
  KV,
  type Tone,
} from './tray/primitives';
import {
  ageLabel,
  fmtBytes,
  fmtDate,
  fmtTime,
  toneForType,
  TONE_VAR,
} from './tray/format';

const REFRESH_MS = 10_000;

/// How many recent events to pull for the Logs view. Matches the
/// `MAX_TIMELINE_LIMIT` ceiling in `commands.rs` minus a margin so
/// future tightening server-side doesn't silently truncate this view.
/// Bigger than StatusPane's default (50) because Logs is the deep-dive
/// surface: users expect to see a session's worth of events here.
const LOGS_TIMELINE_LIMIT = 1_000;

interface DayGroup<T> {
  label: string;
  items: T[];
}

/** Bucket events by their local-day timestamp. Returns groups in
 * insertion order, which (because the timeline is newest-first) means
 * "Today" → "Yesterday" → older days. */
function groupByDay<T extends { timestamp: string }>(
  events: T[],
): DayGroup<T>[] {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const todayKey = today.getTime();

  const groups = new Map<number, DayGroup<T>>();
  for (const e of events) {
    const d = new Date(e.timestamp);
    if (Number.isNaN(d.getTime())) continue;
    d.setHours(0, 0, 0, 0);
    const key = d.getTime();
    let label: string;
    if (key === todayKey) label = 'Today';
    else if (key === todayKey - 86_400_000) label = 'Yesterday';
    else
      label = d.toLocaleDateString(undefined, {
        weekday: 'short',
        month: 'short',
        day: 'numeric',
      });
    let bucket = groups.get(key);
    if (!bucket) {
      bucket = { label, items: [] };
      groups.set(key, bucket);
    }
    bucket.items.push(e);
  }
  return [...groups.values()];
}

interface TypePillProps {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
}

function TypePill({ label, count, active, onClick }: TypePillProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        background: active ? 'var(--accent-soft)' : 'var(--surface)',
        color: active ? 'var(--accent)' : 'var(--fg-muted)',
        border: `1px solid ${active ? 'var(--accent)' : 'var(--border)'}`,
        borderRadius: 'var(--r-pill)',
        padding: '3px 9px',
        fontFamily: 'var(--font-mono)',
        fontSize: 11,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
        transition: 'all 120ms',
      }}
    >
      <span>{label}</span>
      <span
        style={{
          fontSize: 10,
          color: active ? 'var(--accent)' : 'var(--fg-dim)',
          fontVariantNumeric: 'tabular-nums',
        }}
      >
        {count}
      </span>
    </button>
  );
}

const SEARCH_BAR_STYLE: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  background: 'var(--surface)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--r-md)',
  padding: '6px 10px',
};

const SEARCH_INPUT_STYLE: CSSProperties = {
  flex: 1,
  background: 'transparent',
  border: 'none',
  outline: 'none',
  color: 'var(--fg)',
  fontFamily: 'var(--font-mono)',
  fontSize: 12,
  padding: '4px 0',
};

const SCRIM_STYLE: CSSProperties = {
  position: 'fixed',
  inset: 0,
  background: 'rgba(0, 0, 0, 0.55)',
  zIndex: 100,
  display: 'flex',
  alignItems: 'flex-end',
  justifyContent: 'center',
  animation: 'fadeIn 180ms',
};

const DRAWER_STYLE: CSSProperties = {
  width: '100%',
  maxWidth: 720,
  background: 'var(--bg-elev)',
  borderTop: '1px solid var(--border-strong)',
  borderTopLeftRadius: 'var(--r-lg)',
  borderTopRightRadius: 'var(--r-lg)',
  padding: 20,
  maxHeight: '70vh',
  overflowY: 'auto',
  animation: 'slideUp 240ms var(--ease-out)',
};

export function LogsPane() {
  const [entries, setEntries] = useState<TimelineEntry[] | null>(null);
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [query, setQuery] = useState('');
  const [activeType, setActiveType] = useState<string>('all');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [showRaw, setShowRaw] = useState(false);
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>(
    'idle',
  );
  const [noiseError, setNoiseError] = useState<string | null>(null);
  const [pendingNoise, setPendingNoise] = useState(false);
  const [retryState, setRetryState] = useState<'idle' | 'kicked' | 'failed'>(
    'idle',
  );
  const [retryPending, setRetryPending] = useState(false);

  // Single fetch implementation. The optional `signal` is how the
  // mount-time poller cancels in-flight requests on unmount; explicit
  // `refresh()` calls (e.g. after mark-as-noise) skip it because the
  // component is still mounted by definition.
  const fetchAndApply = useCallback(
    async (signal?: { aborted: boolean }): Promise<void> => {
      try {
        const [tl, st] = await Promise.all([
          api.getSessionTimeline(LOGS_TIMELINE_LIMIT),
          api.getStorageStats(),
        ]);
        if (signal?.aborted) return;
        setEntries(tl);
        setStats(st);
      } catch (err) {
        // Stay quiet on the UI — surface to the dev console only,
        // matching StatusPane's behaviour.
        // eslint-disable-next-line no-console
        console.warn('Failed to refresh logs data', err);
      }
    },
    [],
  );

  useEffect(() => {
    const signal = { aborted: false };
    void fetchAndApply(signal);
    const handle = window.setInterval(() => {
      void fetchAndApply(signal);
    }, REFRESH_MS);
    return () => {
      signal.aborted = true;
      window.clearInterval(handle);
    };
  }, [fetchAndApply]);

  // Esc-to-close for the detail drawer. We bind the listener only
  // while a row is selected so we don't intercept Escape elsewhere.
  useEffect(() => {
    if (selectedId === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSelectedId(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selectedId]);

  // Reset transient drawer state when a different row is selected
  // (or the drawer is closed).
  useEffect(() => {
    setShowRaw(false);
    setCopyState('idle');
    setNoiseError(null);
    setRetryState('idle');
  }, [selectedId]);

  const safeEntries = entries ?? [];
  const totalCount = safeEntries.length;
  const unsyncedCount = safeEntries.filter((e) => !e.synced).length;
  const syncedCount = totalCount - unsyncedCount;

  const allTypes = useMemo<[string, number][]>(() => {
    const m = new Map<string, number>();
    for (const e of safeEntries) {
      m.set(e.event_type, (m.get(e.event_type) ?? 0) + 1);
    }
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  }, [safeEntries]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return safeEntries.filter((e) => {
      if (activeType !== 'all' && e.event_type !== activeType) return false;
      if (
        q &&
        !(
          e.event_type.toLowerCase().includes(q) ||
          e.summary.toLowerCase().includes(q)
        )
      )
        return false;
      return true;
    });
  }, [safeEntries, query, activeType]);

  const grouped = useMemo(() => groupByDay(filtered), [filtered]);

  const selected = useMemo(
    () => safeEntries.find((e) => e.id === selectedId) ?? null,
    [safeEntries, selectedId],
  );

  // Stored / synced/ pending pills — we prefer the storage_stats
  // total when available since it counts the full table, not just
  // the most-recent-50 window we render. Synced/Pending are still
  // window-scoped because they require the per-row id comparison
  // and we don't carry that across the full table yet.
  const storedDisplay = stats
    ? stats.total_events.toLocaleString()
    : totalCount.toLocaleString();
  const dbSizeDisplay = stats ? fmtBytes(stats.db_size_bytes) : '—';

  const pendingTone: Tone = unsyncedCount > 0 ? 'warn' : 'default';

  const handleCopyRaw = async () => {
    if (!selected) return;
    try {
      await navigator.clipboard.writeText(selected.raw_line);
      setCopyState('copied');
      window.setTimeout(() => setCopyState('idle'), 1500);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn('Failed to copy raw line', err);
      setCopyState('failed');
      window.setTimeout(() => setCopyState('idle'), 1500);
    }
  };

  const handleMarkAsNoise = async () => {
    if (!selected) return;
    setPendingNoise(true);
    setNoiseError(null);
    try {
      await api.markEventAsNoise(selected.event_type);
      setSelectedId(null);
      await fetchAndApply();
    } catch (err) {
      const message = err instanceof Error ? err.message : 'unknown error';
      setNoiseError(message);
    } finally {
      setPendingNoise(false);
    }
  };

  const handleRetrySync = async () => {
    setRetryPending(true);
    try {
      await api.retrySyncNow();
      setRetryState('kicked');
      // Give the worker a beat to drain before refetching, otherwise
      // the user clicks Retry and sees the same Pending count for a
      // moment because we lapped the worker's loop.
      window.setTimeout(() => {
        void fetchAndApply();
      }, 800);
      window.setTimeout(() => setRetryState('idle'), 2500);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn('Failed to kick sync worker', err);
      setRetryState('failed');
      window.setTimeout(() => setRetryState('idle'), 2500);
    } finally {
      setRetryPending(false);
    }
  };

  const showLoadingState = entries === null;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* HEADLINE STAT STRIP */}
      <div style={{ display: 'flex', gap: 8 }}>
        <StatPill label="Stored" value={storedDisplay} tone="accent" />
        <StatPill
          label="Synced"
          value={syncedCount.toLocaleString()}
          tone="ok"
        />
        <StatPill
          label="Pending"
          value={unsyncedCount.toLocaleString()}
          tone={pendingTone}
        />
        <StatPill label="DB size" value={dbSizeDisplay} />
      </div>

      {/* SEARCH BAR */}
      <div style={SEARCH_BAR_STYLE}>
        <span
          style={{
            color: 'var(--fg-dim)',
            fontSize: 13,
            fontFamily: 'var(--font-mono)',
          }}
        >
          ⌕
        </span>
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter by type or summary…"
          style={SEARCH_INPUT_STYLE}
        />
        {query && (
          <button
            type="button"
            onClick={() => setQuery('')}
            style={{
              background: 'transparent',
              border: 'none',
              color: 'var(--fg-dim)',
              cursor: 'pointer',
              fontSize: 11,
              padding: 0,
              fontFamily: 'inherit',
            }}
          >
            clear
          </button>
        )}
        <span
          style={{
            color: 'var(--fg-dim)',
            fontSize: 11,
            fontFamily: 'var(--font-mono)',
            paddingLeft: 8,
            borderLeft: '1px solid var(--border)',
          }}
        >
          {filtered.length} / {totalCount}
        </span>
      </div>

      {/* TYPE PILL ROW */}
      {allTypes.length > 0 && (
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            gap: 4,
            marginTop: -4,
          }}
        >
          <TypePill
            label="All"
            count={totalCount}
            active={activeType === 'all'}
            onClick={() => setActiveType('all')}
          />
          {allTypes.map(([type, count]) => (
            <TypePill
              key={type}
              label={type}
              count={count}
              active={activeType === type}
              onClick={() => setActiveType(type)}
            />
          ))}
        </div>
      )}

      {/* GROUPED LIST */}
      {showLoadingState ? (
        <TrayCard>
          <p
            style={{
              margin: 0,
              color: 'var(--fg-dim)',
              fontSize: 13,
              textAlign: 'center',
              padding: '12px 0',
            }}
          >
            Loading events…
          </p>
        </TrayCard>
      ) : grouped.length === 0 ? (
        <TrayCard>
          <p
            style={{
              margin: 0,
              color: 'var(--fg-dim)',
              fontSize: 13,
              textAlign: 'center',
              padding: '12px 0',
            }}
          >
            Scope is clear. No events match this filter.
          </p>
        </TrayCard>
      ) : (
        grouped.map((g) => (
          <TrayCard
            key={g.label}
            title={g.label}
            kicker={`${g.items.length} events`}
          >
            <ol
              style={{
                listStyle: 'none',
                margin: 0,
                padding: 0,
                display: 'flex',
                flexDirection: 'column',
                gap: 1,
              }}
            >
              {g.items.map((e) => {
                const tone = toneForType(e.event_type);
                const isSelected = selectedId === e.id;
                return (
                  <li
                    key={e.id}
                    onClick={() => setSelectedId(e.id)}
                    style={{
                      display: 'grid',
                      gridTemplateColumns: '60px 170px 1fr auto',
                      gap: 10,
                      alignItems: 'baseline',
                      padding: '5px 8px',
                      borderLeft: `2px solid ${TONE_VAR[tone]}`,
                      fontSize: 12,
                      cursor: 'pointer',
                      background: isSelected
                        ? 'var(--surface-2)'
                        : 'transparent',
                      transition: 'background 120ms',
                    }}
                    onMouseEnter={(ev) => {
                      if (!isSelected)
                        ev.currentTarget.style.background = 'var(--surface-2)';
                    }}
                    onMouseLeave={(ev) => {
                      if (!isSelected)
                        ev.currentTarget.style.background = 'transparent';
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
                        color: TONE_VAR[tone],
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
                    <span
                      style={{
                        color: 'var(--fg)',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {e.summary}
                    </span>
                    <span
                      style={{
                        fontSize: 10,
                        fontFamily: 'var(--font-mono)',
                        color: e.synced ? 'var(--fg-dim)' : 'var(--warn)',
                        letterSpacing: '0.06em',
                        textTransform: 'uppercase',
                      }}
                      title={e.synced ? 'Synced to remote' : 'Pending sync'}
                    >
                      {e.synced ? '✓' : '↑'}
                    </span>
                  </li>
                );
              })}
            </ol>
          </TrayCard>
        ))
      )}

      {/* DETAIL DRAWER */}
      {selected && (
        <div onClick={() => setSelectedId(null)} style={SCRIM_STYLE}>
          <div
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            style={DRAWER_STYLE}
          >
            <div
              style={{
                display: 'flex',
                alignItems: 'flex-start',
                justifyContent: 'space-between',
                gap: 12,
                marginBottom: 14,
              }}
            >
              <div>
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    marginBottom: 4,
                  }}
                >
                  <StatusDot tone={toneForType(selected.event_type)} />
                  <code
                    style={{
                      color: TONE_VAR[toneForType(selected.event_type)],
                      fontFamily: 'var(--font-mono)',
                      fontSize: 14,
                      fontWeight: 600,
                      textTransform: 'uppercase',
                      letterSpacing: '0.04em',
                    }}
                  >
                    {selected.event_type}
                  </code>
                </div>
                <div style={{ fontSize: 14, color: 'var(--fg)' }}>
                  {selected.summary}
                </div>
              </div>
              <button
                type="button"
                onClick={() => setSelectedId(null)}
                style={{
                  background: 'transparent',
                  border: '1px solid var(--border-strong)',
                  color: 'var(--fg-muted)',
                  borderRadius: 'var(--r-sm)',
                  padding: '4px 10px',
                  fontSize: 12,
                  cursor: 'pointer',
                  fontFamily: 'inherit',
                }}
              >
                Close
              </button>
            </div>

            <dl
              style={{
                display: 'grid',
                gridTemplateColumns: '120px 1fr',
                gap: '6px 12px',
                margin: '0 0 14px',
              }}
            >
              <KV label="Event ID" value={`#${selected.id}`} mono />
              <KV
                label="Captured"
                value={`${fmtDate(selected.timestamp)} · ${fmtTime(
                  selected.timestamp,
                )} (${ageLabel(selected.timestamp)})`}
                mono
              />
              <KV
                label="Sync state"
                value={
                  <span
                    style={{
                      display: 'inline-flex',
                      alignItems: 'center',
                      gap: 6,
                      fontFamily: 'var(--font-mono)',
                      fontSize: 12,
                      color: selected.synced ? 'var(--ok)' : 'var(--warn)',
                    }}
                  >
                    <StatusDot tone={selected.synced ? 'ok' : 'warn'} />
                    {selected.synced
                      ? 'Synced to remote'
                      : 'Pending — will retry next batch'}
                  </span>
                }
              />
              <KV
                label="Source"
                value={`${selected.log_source.toUpperCase()}/Game.log`}
                mono
                dim
              />
            </dl>

            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                marginBottom: 6,
              }}
            >
              <div
                style={{
                  fontSize: 10,
                  fontWeight: 600,
                  color: 'var(--fg-muted)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.12em',
                }}
              >
                Raw line
              </div>
              <button
                type="button"
                onClick={() => setShowRaw((v) => !v)}
                style={{
                  background: 'transparent',
                  border: '1px solid var(--border-strong)',
                  color: 'var(--fg-muted)',
                  borderRadius: 'var(--r-xs)',
                  padding: '2px 8px',
                  fontSize: 10,
                  cursor: 'pointer',
                  fontFamily: 'inherit',
                }}
              >
                {showRaw ? 'Hide' : 'Show'}
              </button>
            </div>
            {showRaw && (
              <pre
                style={{
                  margin: 0,
                  padding: '10px 12px',
                  background: 'var(--bg)',
                  border: '1px solid var(--border)',
                  borderRadius: 'var(--r-sm)',
                  color: 'var(--fg-muted)',
                  fontSize: 11,
                  fontFamily: 'var(--font-mono)',
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-all',
                }}
              >
                {selected.raw_line}
              </pre>
            )}

            {noiseError && (
              <p
                role="alert"
                style={{
                  margin: '10px 0 0',
                  fontSize: 12,
                  color: 'var(--danger)',
                }}
              >
                Couldn&apos;t mark as noise: {noiseError}
              </p>
            )}

            <div
              style={{
                display: 'flex',
                gap: 8,
                marginTop: 14,
                paddingTop: 14,
                borderTop: '1px solid var(--border)',
              }}
            >
              <GhostButton type="button" onClick={handleCopyRaw}>
                {copyState === 'copied'
                  ? 'Copied'
                  : copyState === 'failed'
                    ? 'Copy failed'
                    : 'Copy raw line'}
              </GhostButton>
              <GhostButton
                type="button"
                onClick={handleMarkAsNoise}
                disabled={pendingNoise}
              >
                {pendingNoise ? 'Marking…' : 'Mark as noise'}
              </GhostButton>
              {!selected.synced && (
                <GhostButton
                  type="button"
                  onClick={handleRetrySync}
                  disabled={retryPending}
                  title="Wake the sync worker now instead of waiting for the next tick"
                >
                  {retryState === 'kicked'
                    ? 'Sync nudged'
                    : retryState === 'failed'
                      ? 'Retry failed'
                      : retryPending
                        ? 'Nudging…'
                        : 'Retry sync'}
                </GhostButton>
              )}
            </div>
          </div>
        </div>
      )}

      <style>{`
        @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
        @keyframes slideUp {
          from { transform: translateY(20px); opacity: 0; }
          to { transform: translateY(0); opacity: 1; }
        }
      `}</style>
    </div>
  );
}
