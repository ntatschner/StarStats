/**
 * Journey · Types tab.
 *
 * Migrated from the now-deprecated `/metrics` page per design audit
 * v2 §07 ("metrics is just one more dimensional cut and belongs as
 * a Journey tab"). Combines what used to be the Types breakdown
 * table and the Raw-stream type filter into a single drill surface:
 *
 *  - No `?type=` filter      → ranked table of all event types in
 *                              the active range; rows link into
 *                              themselves with the type pinned.
 *  - With `?type=<name>`     → filtered raw event stream for that
 *                              type, with the existing seq cursor
 *                              pagination contract preserved.
 *
 * The hide/unhide server action moves with the raw rows so the
 * sharing-visibility contract is unchanged for callers.
 */
import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import { revalidatePath } from 'next/cache';
import {
  ApiCallError,
  getMetricsEventTypes,
  hideEvent,
  listEvents,
  unhideEvent,
  type EventDto,
  type EventTypeBreakdownResponse,
  type EventTypeStatsDto,
  type ListEventsResponse,
  type MetricsRange,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { formatEventSummary } from '@/lib/event-summary';
import { formatEventType } from '@/lib/event-types';
import {
  EMPTY_REFERENCE_LOOKUP,
  loadAllReferences,
  type ReferenceLookup,
} from '@/lib/reference';
import { getSession } from '@/lib/session';

const RAW_PAGE_LIMIT = 50;

const RANGE_IDS: MetricsRange[] = ['7d', '30d', '90d', 'all'];

export function parseMetricsRange(raw: string | undefined): MetricsRange {
  if (raw && (RANGE_IDS as readonly string[]).includes(raw)) {
    return raw as MetricsRange;
  }
  return '30d';
}

function parseNonNegInt(raw: string | undefined): number | undefined {
  if (raw === undefined) return undefined;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? n : undefined;
}

// -- Hrefs ----------------------------------------------------------
//
// All links stay inside `/journey?view=types`. The legacy
// `/metrics?view=raw&type=…` URL is preserved by the redirect stub
// at `app/metrics/page.tsx`.

function typesHref(extra?: Record<string, string>): Route {
  const qs = new URLSearchParams({ view: 'types', ...(extra ?? {}) });
  return `/journey?${qs.toString()}` as Route;
}

function typesFilterHref(opts: {
  type?: string;
  before_seq?: number;
  after_seq?: number;
  range?: MetricsRange;
}): Route {
  const qs = new URLSearchParams();
  qs.set('view', 'types');
  if (opts.range) qs.set('range', opts.range);
  if (opts.type) qs.set('type', opts.type);
  // Cursor params are mutually exclusive — never both at once.
  if (opts.before_seq !== undefined) qs.set('before_seq', String(opts.before_seq));
  else if (opts.after_seq !== undefined) qs.set('after_seq', String(opts.after_seq));
  return `/journey?${qs.toString()}` as Route;
}

// -- Tab entry ------------------------------------------------------

export async function TypesTab({
  token,
  range,
  eventType,
  beforeSeqRaw,
  afterSeqRaw,
}: {
  token: string;
  range: MetricsRange;
  eventType?: string;
  beforeSeqRaw?: string;
  afterSeqRaw?: string;
}) {
  const beforeSeq = parseNonNegInt(beforeSeqRaw);
  const afterSeq = parseNonNegInt(afterSeqRaw);
  const isFiltered = eventType !== undefined;

  // Fetch either the ranked breakdown OR the filtered stream, never
  // both. The two views never appear on screen simultaneously and
  // each carries its own auth/error envelope.
  if (isFiltered) {
    let rawEvents: ListEventsResponse | null = null;
    let references: ReferenceLookup = EMPTY_REFERENCE_LOOKUP;
    try {
      [rawEvents, references] = await Promise.all([
        listEvents(token, {
          limit: RAW_PAGE_LIMIT,
          event_type: eventType,
          before_seq: beforeSeq,
          after_seq: afterSeq,
        }),
        loadAllReferences().catch(() => EMPTY_REFERENCE_LOOKUP),
      ]);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        redirect('/auth/login?next=/journey?view=types');
      }
      throw e;
    }
    return (
      <FilteredStream
        events={rawEvents}
        eventType={eventType}
        beforeSeq={beforeSeq}
        afterSeq={afterSeq}
        references={references}
        range={range}
        toggleHide={toggleHideAction}
      />
    );
  }

  let breakdown: EventTypeBreakdownResponse | null = null;
  try {
    breakdown = await getMetricsEventTypes(token, range);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey?view=types');
    }
    throw e;
  }
  return <TypesBreakdown range={range} breakdown={breakdown} />;
}

// -- Server action ----------------------------------------------------
//
// Hide/unhide toggle for raw rows. Same shape as the old
// `/metrics` page; revalidates `/journey` so the next render reflects
// whatever the server actually persisted.
async function toggleHideAction(formData: FormData) {
  'use server';
  const s = await getSession();
  if (!s) redirect('/auth/login?next=/journey?view=types');
  const seqRaw = String(formData.get('seq') ?? '');
  const seq = Number.parseInt(seqRaw, 10);
  if (!Number.isFinite(seq) || seq <= 0) {
    logger.warn({ seqRaw }, 'TypesTab toggleHide: malformed seq');
    return;
  }
  const currentlyHidden =
    String(formData.get('currently_hidden') ?? '') === 'true';
  try {
    if (currentlyHidden) {
      await unhideEvent(s.token, seq);
    } else {
      await hideEvent(s.token, seq);
    }
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey?view=types');
    }
    logger.error(
      { err: e, seq, currentlyHidden },
      'TypesTab hide toggle failed',
    );
  }
  revalidatePath('/journey');
}

// -- Breakdown view -------------------------------------------------

function TypesBreakdown({
  range,
  breakdown,
}: {
  range: MetricsRange;
  breakdown: EventTypeBreakdownResponse | null;
}) {
  const types = breakdown?.types ?? [];
  const total = types.reduce((acc, t) => acc + t.count, 0);
  const sorted = [...types].sort((a, b) => b.count - a.count);

  return (
    <section className="ss-card">
      <header
        style={{
          padding: '20px 24px 0',
          display: 'flex',
          alignItems: 'baseline',
          justifyContent: 'space-between',
          gap: 14,
          flexWrap: 'wrap',
        }}
      >
        <div>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Event types · {types.length} distinct
          </div>
          <h2
            style={{
              margin: 0,
              fontSize: 17,
              fontWeight: 600,
              letterSpacing: '-0.01em',
            }}
          >
            What the client has classified
          </h2>
        </div>
        <TypesRangeSwitcher current={range} />
      </header>
      <div style={{ padding: '16px 24px 22px' }}>
        {sorted.length === 0 ? (
          <EmptyLine />
        ) : (
          <div className="ss-table-wrap">
            <table className="ss-table" style={{ fontSize: 13 }}>
              <thead>
                <tr>
                  <th style={{ textAlign: 'left' }}>Type</th>
                  <th style={{ textAlign: 'right' }}>Count</th>
                  <th style={{ textAlign: 'right' }}>% of total</th>
                  <th style={{ textAlign: 'right' }}>Last seen</th>
                  {/* 30d trend signal isn't computed by the backend yet
                      — column reserved so the layout stays stable
                      once it lands. */}
                  <th style={{ textAlign: 'right' }}>30d trend</th>
                </tr>
              </thead>
              <tbody>
                {sorted.map((t) => (
                  <TypeRow key={t.event_type} t={t} total={total} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </section>
  );
}

function TypeRow({ t, total }: { t: EventTypeStatsDto; total: number }) {
  const pct = total > 0 ? (t.count / total) * 100 : 0;
  return (
    <tr>
      <td>
        <Link
          href={typesFilterHref({ type: t.event_type })}
          title={t.event_type}
          style={{
            color: 'var(--accent)',
            textDecoration: 'none',
            display: 'inline-flex',
            gap: 6,
            alignItems: 'baseline',
          }}
        >
          <span aria-hidden="true">
            {formatEventType(t.event_type).glyph}
          </span>
          <span>{formatEventType(t.event_type).label}</span>
        </Link>
      </td>
      <td style={{ textAlign: 'right' }} className="mono">
        {t.count.toLocaleString()}
      </td>
      <td
        style={{ textAlign: 'right', color: 'var(--fg-muted)' }}
        className="mono"
      >
        {pct.toFixed(1)}%
      </td>
      <td
        style={{ textAlign: 'right', color: 'var(--fg-muted)' }}
        className="mono"
      >
        {formatLastSeen(t.last_seen ?? null)}
      </td>
      <td
        style={{ textAlign: 'right', color: 'var(--fg-dim)' }}
        className="mono"
      >
        —
      </td>
    </tr>
  );
}

function TypesRangeSwitcher({ current }: { current: MetricsRange }) {
  // Note: this is the metrics-style range (7d/30d/90d/all) — distinct
  // from the journey-wide hour-based RangeBar. The Types tab needs the
  // backend's metrics-range tokens because `getMetricsEventTypes`
  // accepts only this enum, not raw hours.
  const labels: Record<MetricsRange, string> = {
    '7d': '7 days',
    '30d': '30 days',
    '90d': '90 days',
    all: 'All time',
  };
  return (
    <div style={{ display: 'inline-flex', gap: 4, flexWrap: 'wrap' }}>
      {RANGE_IDS.map((id) => {
        const active = id === current;
        return (
          <Link
            key={id}
            href={typesHref({ range: id })}
            data-active={active ? 'true' : undefined}
            style={{
              background: active ? 'var(--bg-elev)' : 'transparent',
              border: '1px solid',
              borderColor: active ? 'var(--border-strong)' : 'var(--border)',
              color: active ? 'var(--fg)' : 'var(--fg-muted)',
              padding: '4px 10px',
              borderRadius: 'var(--r-pill)',
              fontSize: 12,
              textDecoration: 'none',
            }}
          >
            {labels[id]}
          </Link>
        );
      })}
    </div>
  );
}

// -- Filtered raw stream --------------------------------------------

function FilteredStream({
  events,
  eventType,
  beforeSeq,
  afterSeq,
  references,
  range,
  toggleHide,
}: {
  events: ListEventsResponse | null;
  eventType: string;
  beforeSeq: number | undefined;
  afterSeq: number | undefined;
  references: ReferenceLookup;
  range: MetricsRange;
  toggleHide: (formData: FormData) => Promise<void>;
}) {
  const rows = events?.events ?? [];
  // Sort newest-first; the server's DESC default already does this
  // for unparametrised calls, but defensively re-sort here so cursor
  // pages stay consistent.
  const sorted = [...rows].sort((a, b) => b.seq - a.seq);
  const newestSeq = sorted.length ? sorted[0].seq : null;
  const oldestSeq = sorted.length ? sorted[sorted.length - 1].seq : null;

  const hasCursor = beforeSeq !== undefined || afterSeq !== undefined;
  const showOlder = sorted.length === RAW_PAGE_LIMIT && oldestSeq !== null;
  const showNewer = hasCursor && newestSeq !== null;

  const olderHref = oldestSeq !== null
    ? typesFilterHref({ type: eventType, before_seq: oldestSeq, range })
    : null;
  const newerHref = newestSeq !== null
    ? typesFilterHref({ type: eventType, after_seq: newestSeq, range })
    : null;
  const clearHref = typesHref({ range });

  return (
    <section className="ss-card">
      <header style={{ padding: '20px 24px 0' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Filtered events · last {sorted.length || RAW_PAGE_LIMIT}
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          Every line as parsed
        </h2>
      </header>
      <div style={{ padding: '16px 24px 22px' }}>
        <div style={{ marginBottom: 12 }}>
          <span className="ss-badge ss-badge--accent" title={eventType}>
            Filter:{' '}
            <span style={{ marginLeft: 6 }}>
              {formatEventType(eventType).glyph}{' '}
              {formatEventType(eventType).label}
            </span>
          </span>{' '}
          <Link
            href={clearHref}
            aria-label="Clear filter"
            style={{
              color: 'var(--fg-dim)',
              fontSize: 12,
              marginLeft: 8,
            }}
          >
            × clear
          </Link>
        </div>
        {sorted.length === 0 ? (
          <EmptyLine text="Scope is clear. No events match this filter." />
        ) : (
          <ol
            style={{
              listStyle: 'none',
              margin: 0,
              padding: 0,
              display: 'flex',
              flexDirection: 'column',
            }}
          >
            {sorted.map((e, i) => (
              <RawRow
                key={e.seq}
                e={e}
                last={i === sorted.length - 1}
                references={references}
                toggleHide={toggleHide}
              />
            ))}
          </ol>
        )}
      </div>
      {(showOlder || showNewer) && (
        <>
          <hr className="ss-rule" />
          <nav
            aria-label="Filtered stream pagination"
            style={{
              padding: '14px 24px',
              display: 'flex',
              justifyContent: 'space-between',
              gap: 12,
              fontSize: 13,
            }}
          >
            {showNewer && newerHref ? (
              <Link
                href={newerHref}
                className="ss-btn ss-btn--link"
                style={{ background: 'transparent' }}
              >
                ← Newer
              </Link>
            ) : (
              <span style={{ color: 'var(--fg-dim)' }}>← Newer</span>
            )}
            {showOlder && olderHref ? (
              <Link
                href={olderHref}
                className="ss-btn ss-btn--link"
                style={{ background: 'transparent' }}
              >
                Older →
              </Link>
            ) : (
              <span style={{ color: 'var(--fg-dim)' }}>Older →</span>
            )}
          </nav>
        </>
      )}
    </section>
  );
}

function RawRow({
  e,
  last,
  references,
  toggleHide,
}: {
  e: EventDto;
  last: boolean;
  references: ReferenceLookup;
  toggleHide: (formData: FormData) => Promise<void>;
}) {
  const hidden = e.hidden_at !== null && e.hidden_at !== undefined;
  return (
    <li
      style={{
        display: 'grid',
        // timestamp · event-type · summary · hide-toggle.
        gridTemplateColumns: '120px 200px 1fr 110px',
        gap: 14,
        alignItems: 'baseline',
        padding: '10px 0',
        borderBottom: last ? 'none' : '1px solid var(--border)',
        fontFamily: 'var(--font-mono)',
        fontSize: 12,
        opacity: hidden ? 0.5 : 1,
      }}
    >
      <span style={{ color: 'var(--fg-dim)' }}>
        {formatTimeFull(e.event_timestamp)}
      </span>
      <Link
        href={typesFilterHref({ type: e.event_type })}
        title={e.event_type}
        style={{
          color: 'var(--accent)',
          textDecoration: 'none',
          display: 'inline-flex',
          gap: 6,
          alignItems: 'baseline',
        }}
      >
        <span aria-hidden="true">
          {formatEventType(e.event_type).glyph}
        </span>
        <span>{formatEventType(e.event_type).label}</span>
      </Link>
      <span
        style={{
          color: 'var(--fg-muted)',
          textDecoration: hidden ? 'line-through' : undefined,
        }}
      >
        {formatEventSummary(e.payload, references)}
      </span>
      <form
        action={toggleHide}
        style={{
          margin: 0,
          display: 'flex',
          justifyContent: 'flex-end',
          alignItems: 'center',
          gap: 6,
        }}
      >
        <input type="hidden" name="seq" value={String(e.seq)} />
        <input
          type="hidden"
          name="currently_hidden"
          value={hidden ? 'true' : 'false'}
        />
        {hidden && (
          <span
            className="ss-badge"
            title={`Hidden from shares at ${e.hidden_at}`}
            style={{
              fontSize: 10,
              borderColor: 'var(--fg-dim)',
              color: 'var(--fg-dim)',
            }}
          >
            hidden
          </span>
        )}
        <button
          type="submit"
          className="ss-btn ss-btn--link"
          title={
            hidden
              ? 'Make this event visible to people you share with again'
              : 'Hide this event from shared and public views'
          }
          style={{
            fontSize: 11,
            color: hidden ? 'var(--accent)' : 'var(--fg-muted)',
          }}
        >
          {hidden ? 'Unhide' : 'Hide'}
        </button>
      </form>
    </li>
  );
}

// -- Helpers --------------------------------------------------------

function EmptyLine({ text = 'Scope is clear.' }: { text?: string } = {}) {
  return (
    <p style={{ margin: 0, color: 'var(--fg-muted)', fontSize: 13 }}>{text}</p>
  );
}

function formatTimeFull(iso: string | null): string {
  if (!iso) return '—';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleTimeString();
}

/**
 * Relative-ish "last seen" stamp used on the breakdown table.
 *  - <1 min  → "now"
 *  - <60 min → "{n}m ago"
 *  - <24 h   → "{n}h ago"
 *  - <7 days → "{n}d ago"
 *  - older   → "Mmm dd"
 */
function formatLastSeen(iso: string | null): string {
  if (!iso) return '—';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '—';
  const diffMs = Date.now() - d.getTime();
  if (diffMs < 60_000) return 'now';
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}
