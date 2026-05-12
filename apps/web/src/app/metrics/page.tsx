/**
 * Metrics page — 4-tab manifest viewer powered by:
 *   - GET /v1/me/summary           (totals + by_type for header pills)
 *   - GET /v1/me/timeline          (overview activity heatmap-adjacent)
 *   - GET /v1/me/metrics/event-types?range=...   (Event types tab)
 *   - GET /v1/me/metrics/sessions?limit&offset   (Sessions tab + recent strip)
 *   - GET /v1/me/events?limit&before_seq&...     (Raw stream tab)
 *
 * Tab + range + cursor selection is fully URL-driven (search params)
 * so this is a pure server component — no client JS needed for nav.
 *
 * Mirrors the auth + error-handling pattern of `app/dashboard/page.tsx`.
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getMetricsEventTypes,
  getMetricsSessions,
  getSummary,
  getTimeline,
  listEvents,
  type EventDto,
  type EventTypeBreakdownResponse,
  type EventTypeStatsDto,
  type ListEventsResponse,
  type MetricsRange,
  type SessionDto,
  type SessionsResponse,
  type SummaryResponse,
  type TimelineResponse,
} from '@/lib/api';
import { formatEventSummary } from '@/lib/event-summary';
import { getSession } from '@/lib/session';
import { YearHeatmap } from '@/components/metrics/YearHeatmap';
import { TypeBreakdown } from '@/components/metrics/TypeBreakdown';

const TAB_IDS = ['overview', 'types', 'sessions', 'raw'] as const;
type TabId = (typeof TAB_IDS)[number];

const RANGE_IDS: MetricsRange[] = ['7d', '30d', '90d', 'all'];

const SESSIONS_PAGE_LIMIT = 100;
const RAW_PAGE_LIMIT = 50;
// `getMetricsSessions` is capped server-side at 500. We use that ceiling
// to back the header `sessionCount` pill so it stays roughly accurate
// without an extra round-trip dedicated to a count query.
const HEADER_SESSION_PROBE_LIMIT = 500;

interface SearchParams {
  view?: string;
  range?: string;
  offset?: string;
  type?: string;
  before_seq?: string;
  after_seq?: string;
}

export default async function MetricsPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/metrics');

  const params = await props.searchParams;
  const view = parseTab(params.view);
  const range = parseRange(params.range);
  const offset = parseNonNegInt(params.offset) ?? 0;
  const eventType = params.type;
  const beforeSeq = parseNonNegInt(params.before_seq);
  const afterSeq = parseNonNegInt(params.after_seq);

  // Always-on data (cheap, drives the header strip + overview tiles).
  // Tab-specific calls are conditioned below to avoid wasted round trips
  // — e.g. visiting Raw stream shouldn't fetch the event-types breakdown.
  let summary: SummaryResponse;
  let timeline: TimelineResponse | null = null;
  let typesBreakdown: EventTypeBreakdownResponse | null = null;
  let sessions: SessionsResponse | null = null;
  let recentSessions: SessionsResponse | null = null;
  let rawEvents: ListEventsResponse | null = null;
  let headerSessions: SessionsResponse | null = null;

  try {
    if (view === 'overview') {
      const [s, t, ts, recent, hdr] = await Promise.all([
        getSummary(session.token),
        getTimeline(session.token, { days: 365 }),
        getMetricsEventTypes(session.token, '30d'),
        getMetricsSessions(session.token, { limit: 5 }),
        getMetricsSessions(session.token, { limit: HEADER_SESSION_PROBE_LIMIT }),
      ]);
      summary = s;
      timeline = t;
      typesBreakdown = ts;
      recentSessions = recent;
      headerSessions = hdr;
    } else if (view === 'types') {
      const [s, ts, hdr] = await Promise.all([
        getSummary(session.token),
        getMetricsEventTypes(session.token, range),
        getMetricsSessions(session.token, { limit: HEADER_SESSION_PROBE_LIMIT }),
      ]);
      summary = s;
      typesBreakdown = ts;
      headerSessions = hdr;
    } else if (view === 'sessions') {
      const [s, sess, hdr] = await Promise.all([
        getSummary(session.token),
        getMetricsSessions(session.token, {
          limit: SESSIONS_PAGE_LIMIT,
          offset,
        }),
        getMetricsSessions(session.token, { limit: HEADER_SESSION_PROBE_LIMIT }),
      ]);
      summary = s;
      sessions = sess;
      headerSessions = hdr;
    } else {
      const [s, raw, hdr] = await Promise.all([
        getSummary(session.token),
        listEvents(session.token, {
          limit: RAW_PAGE_LIMIT,
          event_type: eventType,
          before_seq: beforeSeq,
          after_seq: afterSeq,
        }),
        getMetricsSessions(session.token, { limit: HEADER_SESSION_PROBE_LIMIT }),
      ]);
      summary = s;
      rawEvents = raw;
      headerSessions = hdr;
    }
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/metrics');
    }
    throw e;
  }

  const totalEvents = summary.total;
  const distinctTypes = summary.by_type.length;
  const sessionCount = headerSessions?.sessions.length ?? 0;

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Metrics · what the client has captured
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Your manifest
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
          }}
        >
          Every event the desktop client has parsed, indexed by what kind it
          is and when.{' '}
          <span className="mono" style={{ color: 'var(--fg)' }}>
            {totalEvents.toLocaleString()}
          </span>{' '}
          events ·{' '}
          <span className="mono" style={{ color: 'var(--fg)' }}>
            {sessionCount}
          </span>{' '}
          sessions ·{' '}
          <span className="mono" style={{ color: 'var(--fg)' }}>
            {distinctTypes}
          </span>{' '}
          distinct types.
        </p>
      </header>

      <TabNav
        current={view}
        typesCount={distinctTypes}
        sessionsCount={sessionCount}
        rawCount={totalEvents}
      />

      {view === 'overview' && (
        <OverviewTab
          summary={summary}
          timeline={timeline}
          typesBreakdown={typesBreakdown}
          recentSessions={recentSessions}
        />
      )}
      {view === 'types' && (
        <TypesTab range={range} breakdown={typesBreakdown} />
      )}
      {view === 'sessions' && (
        <SessionsTab sessions={sessions} offset={offset} />
      )}
      {view === 'raw' && (
        <RawTab
          events={rawEvents}
          eventType={eventType}
          beforeSeq={beforeSeq}
          afterSeq={afterSeq}
        />
      )}
    </div>
  );
}

// -- Search-param parsing -------------------------------------------

function parseTab(raw: string | undefined): TabId {
  if (raw && (TAB_IDS as readonly string[]).includes(raw)) {
    return raw as TabId;
  }
  return 'overview';
}

function parseRange(raw: string | undefined): MetricsRange {
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

function tabHref(view: TabId, extra?: Record<string, string>): Route {
  const qs = new URLSearchParams({ view, ...(extra ?? {}) });
  return `/metrics?${qs.toString()}` as Route;
}

function rawHref(opts: {
  type?: string;
  before_seq?: number;
  after_seq?: number;
}): Route {
  const qs = new URLSearchParams();
  qs.set('view', 'raw');
  if (opts.type) qs.set('type', opts.type);
  // Mutually exclusive — never include both cursors.
  if (opts.before_seq !== undefined) qs.set('before_seq', String(opts.before_seq));
  else if (opts.after_seq !== undefined) qs.set('after_seq', String(opts.after_seq));
  return `/metrics?${qs.toString()}` as Route;
}

// -- Tab nav --------------------------------------------------------

function TabNav({
  current,
  typesCount,
  sessionsCount,
  rawCount,
}: {
  current: TabId;
  typesCount: number;
  sessionsCount: number;
  rawCount: number;
}) {
  const tabs: Array<{ id: TabId; label: string; count?: number }> = [
    { id: 'overview', label: 'Overview' },
    { id: 'types', label: 'Event types', count: typesCount },
    { id: 'sessions', label: 'Sessions', count: sessionsCount },
    { id: 'raw', label: 'Raw stream', count: rawCount },
  ];
  return (
    <nav
      aria-label="Metrics tabs"
      style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}
    >
      {tabs.map((t) => (
        <TabLink key={t.id} {...t} active={current === t.id} />
      ))}
    </nav>
  );
}

function TabLink({
  id,
  label,
  count,
  active,
}: {
  id: TabId;
  label: string;
  count?: number;
  active: boolean;
}) {
  return (
    <Link
      href={tabHref(id)}
      data-active={active ? 'true' : undefined}
      style={{
        background: active ? 'var(--bg-elev)' : 'transparent',
        border: '1px solid',
        borderColor: active ? 'var(--border-strong)' : 'transparent',
        color: active ? 'var(--fg)' : 'var(--fg-muted)',
        padding: '8px 14px',
        borderRadius: 'var(--r-pill)',
        font: 'inherit',
        fontSize: 13,
        textDecoration: 'none',
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
      }}
    >
      <span>{label}</span>
      {count != null && (
        <span
          className="mono"
          style={{
            fontSize: 11,
            color: 'var(--fg-dim)',
            padding: '2px 6px',
            background: 'var(--bg)',
            borderRadius: 4,
          }}
        >
          {count.toLocaleString()}
        </span>
      )}
    </Link>
  );
}

// -- Overview tab ---------------------------------------------------

function OverviewTab({
  summary,
  timeline,
  typesBreakdown,
  recentSessions,
}: {
  summary: SummaryResponse;
  timeline: TimelineResponse | null;
  typesBreakdown: EventTypeBreakdownResponse | null;
  recentSessions: SessionsResponse | null;
}) {
  const topTypes = [...summary.by_type]
    .sort((a, b) => b.count - a.count)
    .slice(0, 1);
  const topType = topTypes[0];
  const last30Total =
    timeline?.buckets.reduce((acc, b) => acc + b.count, 0) ?? 0;

  // Top 6 from the breakdown (already sorted DESC by count server-side
  // but defensively re-sort so ordering is locked in here too).
  const top6 = (typesBreakdown?.types ?? [])
    .slice()
    .sort((a, b) => b.count - a.count)
    .slice(0, 6);
  const top6Max = top6[0]?.count ?? 0;
  const recent = (recentSessions?.sessions ?? []).slice(0, 5);

  if (summary.total === 0) {
    return (
      <section className="ss-card" style={{ padding: '40px 24px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Empty manifest
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          Scope is clear.
        </h2>
        <p
          style={{
            margin: '10px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 13,
            lineHeight: 1.6,
          }}
        >
          Pair a desktop client on your{' '}
          <Link href="/devices" style={{ color: 'var(--accent)' }}>
            Devices
          </Link>{' '}
          page, launch Star Citizen, and play for a few minutes — events
          will start landing here as the tray drains them upstream.
        </p>
      </section>
    );
  }

  return (
    <>
      {/* Stat strip — derived from the overview's already-loaded data. */}
      <div
        data-rsprow="nowrap"
        style={{ display: 'flex', gap: 12, flexWrap: 'nowrap' }}
      >
        <StatTile
          eyebrow="Total events"
          value={summary.total.toLocaleString()}
          hint={`+${last30Total.toLocaleString()} last 30d`}
          hintKind="ok"
        />
        <StatTile
          eyebrow="Distinct types"
          value={String(summary.by_type.length)}
          hint="signals classified"
        />
        <StatTile
          eyebrow="Sessions"
          value={String(recentSessions?.sessions.length ?? 0)}
          hint="recent"
        />
        <StatTile
          eyebrow="Top type"
          value={topType?.event_type ?? '—'}
          hint={topType ? `${topType.count.toLocaleString()} captures` : ''}
          mono
        />
      </div>

      {/* Year-view activity heatmap — successor to the 30-day grid.
          Reads the same `timeline` data (now 365-day window). */}
      {timeline ? <YearHeatmap timeline={timeline} /> : null}

      {/* Donut + ranked-bar combo replacing the manual bar divs that
          previously occupied the Types tab. Lives on Overview too so
          the breakdown is visible without a tab change. */}
      {typesBreakdown ? (
        <TypeBreakdown
          types={typesBreakdown.types.map((t) => ({
            event_type: t.event_type,
            count: t.count,
          }))}
          caption={`${typesBreakdown.types.length} distinct types`}
        />
      ) : null}

      <div
        data-rspgrid="2"
        style={{
          display: 'grid',
          gridTemplateColumns: '1fr 1fr',
          gap: 16,
          alignItems: 'stretch',
        }}
      >
        <section className="ss-card">
          <header style={{ padding: '20px 24px 0' }}>
            <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
              Top event types · last 30 days
            </div>
            <h2
              style={{
                margin: 0,
                fontSize: 17,
                fontWeight: 600,
                letterSpacing: '-0.01em',
              }}
            >
              What you do most
            </h2>
          </header>
          <div style={{ padding: '16px 24px 22px' }}>
            {top6.length === 0 ? (
              <EmptyLine />
            ) : (
              <ul
                style={{
                  listStyle: 'none',
                  margin: 0,
                  padding: 0,
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 10,
                }}
              >
                {top6.map((t, i) => {
                  const pct = top6Max > 0 ? (t.count / top6Max) * 100 : 0;
                  return (
                    <li
                      key={t.event_type}
                      style={{
                        display: 'grid',
                        gridTemplateColumns: '1fr auto',
                        gap: 6,
                        alignItems: 'baseline',
                      }}
                    >
                      <div
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'baseline',
                          gridColumn: '1 / -1',
                        }}
                      >
                        <span className="mono" style={{ fontSize: 13 }}>
                          {t.event_type}
                        </span>
                        <span
                          className="mono"
                          style={{
                            fontSize: 12,
                            color: 'var(--fg-muted)',
                          }}
                        >
                          {t.count.toLocaleString()}
                        </span>
                      </div>
                      <div
                        style={{
                          gridColumn: '1 / -1',
                          height: 4,
                          background: 'var(--bg)',
                          borderRadius: 2,
                          overflow: 'hidden',
                        }}
                        aria-hidden="true"
                      >
                        <div
                          style={{
                            height: '100%',
                            width: `${pct}%`,
                            background: 'var(--accent)',
                            opacity: 0.55 + (1 - i / 6) * 0.4,
                          }}
                        />
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </section>

        <section className="ss-card">
          <header style={{ padding: '20px 24px 0' }}>
            <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
              Recent sessions
            </div>
            <h2
              style={{
                margin: 0,
                fontSize: 17,
                fontWeight: 600,
                letterSpacing: '-0.01em',
              }}
            >
              Last few drops
            </h2>
          </header>
          <div style={{ padding: '16px 24px 22px' }}>
            {recent.length === 0 ? (
              <EmptyLine />
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column' }}>
                {recent.map((s, i) => (
                  <RecentSessionRow
                    key={`${s.start_at}-${i}`}
                    s={s}
                    last={i === recent.length - 1}
                  />
                ))}
              </div>
            )}
          </div>
        </section>
      </div>
    </>
  );
}

function RecentSessionRow({ s, last }: { s: SessionDto; last: boolean }) {
  const start = new Date(s.start_at);
  const end = new Date(s.end_at);
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: 'auto 1fr auto',
        gap: 14,
        alignItems: 'center',
        padding: '10px 0',
        borderBottom: last ? 'none' : '1px solid var(--border)',
      }}
    >
      <div
        className="mono"
        style={{
          fontSize: 12,
          color: 'var(--fg-muted)',
          minWidth: 90,
        }}
      >
        {formatDateShort(start)}
      </div>
      <div>
        <div className="mono" style={{ fontSize: 11, color: 'var(--fg-dim)' }}>
          {formatHm(start)}–{formatHm(end)} ·{' '}
          {formatDuration(end.getTime() - start.getTime())}
        </div>
      </div>
      <div className="mono" style={{ fontSize: 12, color: 'var(--accent)' }}>
        {s.event_count.toLocaleString()} ev
      </div>
    </div>
  );
}

// -- Event types tab -----------------------------------------------

function TypesTab({
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
        <RangeSwitcher current={range} />
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
                  {/* TODO: 30d trend signal isn't computed by the backend
                      yet — column reserved so the layout stays stable
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
          href={rawHref({ type: t.event_type })}
          className="mono"
          style={{ color: 'var(--accent)', textDecoration: 'none' }}
        >
          {t.event_type}
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

function RangeSwitcher({ current }: { current: MetricsRange }) {
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
            href={tabHref('types', { range: id })}
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

// -- Sessions tab --------------------------------------------------

function SessionsTab({
  sessions,
  offset,
}: {
  sessions: SessionsResponse | null;
  offset: number;
}) {
  const rows = sessions?.sessions ?? [];
  // Pagination heuristic: if we returned a full page assume there's
  // probably another page. The backend doesn't surface a `has_more`
  // cursor on this endpoint yet — simple offset bump is fine.
  const showOlder = rows.length === SESSIONS_PAGE_LIMIT;
  const showNewer = offset > 0;

  return (
    <section className="ss-card">
      <header style={{ padding: '20px 24px 0' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Sessions · {rows.length} on this page
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          When you were flying
        </h2>
      </header>
      <div style={{ padding: '16px 24px 22px' }}>
        {rows.length === 0 ? (
          <EmptyLine />
        ) : (
          <div className="ss-table-wrap">
            <table className="ss-table" style={{ fontSize: 13 }}>
              <thead>
                <tr>
                  <th style={{ textAlign: 'left' }}>When</th>
                  <th style={{ textAlign: 'left' }}>Window</th>
                  <th style={{ textAlign: 'left' }}>Duration</th>
                  {/* TODO: backend doesn't surface a session's primary
                      ship or originating client yet — re-add the
                      `Primary ship` and `Client` columns once the
                      sessions endpoint enriches its rows. */}
                  <th style={{ textAlign: 'right' }}>Events</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((s, i) => (
                  <SessionRow key={`${s.start_at}-${i}`} s={s} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
      {(showOlder || showNewer) && (
        <>
          <hr className="ss-rule" />
          <nav
            aria-label="Sessions pagination"
            style={{
              padding: '14px 24px',
              display: 'flex',
              justifyContent: 'space-between',
              gap: 12,
              fontSize: 13,
            }}
          >
            {showNewer ? (
              <Link
                href={tabHref(
                  'sessions',
                  offset - SESSIONS_PAGE_LIMIT > 0
                    ? { offset: String(offset - SESSIONS_PAGE_LIMIT) }
                    : undefined,
                )}
                className="ss-btn ss-btn--link"
                style={{ background: 'transparent' }}
              >
                ← Newer
              </Link>
            ) : (
              <span style={{ color: 'var(--fg-dim)' }}>← Newer</span>
            )}
            {showOlder ? (
              <Link
                href={tabHref('sessions', {
                  offset: String(offset + SESSIONS_PAGE_LIMIT),
                })}
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

function SessionRow({ s }: { s: SessionDto }) {
  const start = new Date(s.start_at);
  const end = new Date(s.end_at);
  const durationMs = end.getTime() - start.getTime();
  return (
    <tr>
      <td style={{ color: 'var(--fg-muted)' }}>{formatDateShort(start)}</td>
      <td>
        <span className="mono" style={{ fontSize: 12 }}>
          {formatHm(start)}–{formatHm(end)}
        </span>
      </td>
      <td>
        <span className="mono" style={{ fontSize: 12 }}>
          {formatDuration(durationMs)}
        </span>
      </td>
      <td style={{ textAlign: 'right' }}>
        <span className="mono" style={{ color: 'var(--accent)' }}>
          {s.event_count.toLocaleString()}
        </span>
      </td>
    </tr>
  );
}

// -- Raw stream tab ------------------------------------------------

function RawTab({
  events,
  eventType,
  beforeSeq,
  afterSeq,
}: {
  events: ListEventsResponse | null;
  eventType: string | undefined;
  beforeSeq: number | undefined;
  afterSeq: number | undefined;
}) {
  const rows = events?.events ?? [];
  // Sort newest-first to mirror dashboard's stream rendering — the
  // server's DESC default already does this for unparametrised calls,
  // but defensively re-sort here so cursor pages stay consistent.
  const sorted = [...rows].sort((a, b) => b.seq - a.seq);
  const newestSeq = sorted.length ? sorted[0].seq : null;
  const oldestSeq = sorted.length ? sorted[sorted.length - 1].seq : null;

  const hasFilter =
    eventType !== undefined ||
    beforeSeq !== undefined ||
    afterSeq !== undefined;
  const showOlder = sorted.length === RAW_PAGE_LIMIT && oldestSeq !== null;
  const showNewer = hasFilter && newestSeq !== null;

  const olderHref = oldestSeq !== null
    ? rawHref({ type: eventType, before_seq: oldestSeq })
    : null;
  const newerHref = newestSeq !== null
    ? rawHref({ type: eventType, after_seq: newestSeq })
    : null;
  const clearHref = tabHref('raw');

  return (
    <section className="ss-card">
      <header style={{ padding: '20px 24px 0' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Raw event stream · last {sorted.length || RAW_PAGE_LIMIT}
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
        {eventType && (
          <div style={{ marginBottom: 12 }}>
            <span className="ss-badge ss-badge--accent">
              Filter:{' '}
              <span className="mono" style={{ marginLeft: 6 }}>
                type={eventType}
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
        )}
        {sorted.length === 0 ? (
          <EmptyLine
            text={
              hasFilter
                ? 'Scope is clear. No events match this filter.'
                : 'Scope is clear.'
            }
          />
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
              <RawRow key={e.seq} e={e} last={i === sorted.length - 1} />
            ))}
          </ol>
        )}
      </div>
      {(showOlder || showNewer) && (
        <>
          <hr className="ss-rule" />
          <nav
            aria-label="Raw stream pagination"
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

function RawRow({ e, last }: { e: EventDto; last: boolean }) {
  return (
    <li
      style={{
        display: 'grid',
        gridTemplateColumns: '120px 200px 1fr',
        gap: 14,
        alignItems: 'baseline',
        padding: '10px 0',
        borderBottom: last ? 'none' : '1px solid var(--border)',
        fontFamily: 'var(--font-mono)',
        fontSize: 12,
      }}
    >
      <span style={{ color: 'var(--fg-dim)' }}>
        {formatTimeFull(e.event_timestamp)}
      </span>
      <Link
        href={rawHref({ type: e.event_type })}
        style={{ color: 'var(--accent)', textDecoration: 'none' }}
      >
        {e.event_type}
      </Link>
      <span style={{ color: 'var(--fg-muted)' }}>
        {formatEventSummary(e.payload)}
      </span>
    </li>
  );
}

// -- Shared bits ---------------------------------------------------

function StatTile({
  eyebrow,
  value,
  hint,
  hintKind,
  mono = true,
}: {
  eyebrow: string;
  value: string;
  hint?: string;
  hintKind?: 'ok' | 'neutral';
  mono?: boolean;
}) {
  return (
    <div
      className="ss-card"
      style={{ flex: '1 1 200px', padding: '18px 20px', minWidth: 0 }}
    >
      <div className="ss-eyebrow">{eyebrow}</div>
      <div
        className={mono ? 'mono' : ''}
        style={{
          fontSize: 28,
          fontWeight: 500,
          letterSpacing: '-0.015em',
          margin: '8px 0 6px',
          color: 'var(--fg)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {value}
      </div>
      {hint && (
        <div
          style={{
            color: hintKind === 'ok' ? 'var(--ok)' : 'var(--fg-dim)',
            fontSize: 12,
          }}
        >
          {hint}
        </div>
      )}
    </div>
  );
}

function EmptyLine({ text = 'Scope is clear.' }: { text?: string } = {}) {
  return (
    <p style={{ margin: 0, color: 'var(--fg-muted)', fontSize: 13 }}>{text}</p>
  );
}

// -- Date / duration helpers ---------------------------------------

function formatHm(d: Date): string {
  if (Number.isNaN(d.getTime())) return '—';
  const h = String(d.getHours()).padStart(2, '0');
  const m = String(d.getMinutes()).padStart(2, '0');
  return `${h}:${m}`;
}

function formatDateShort(d: Date): string {
  if (Number.isNaN(d.getTime())) return '—';
  // e.g. "Apr 28" — short month + day, no year. Matches the design's
  // recent-sessions stamp style.
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '—';
  const totalMins = Math.round(ms / 60_000);
  const h = Math.floor(totalMins / 60);
  const m = totalMins % 60;
  if (h <= 0) return `${m}m`;
  return `${h}h ${m}m`;
}

function formatTimeFull(iso: string | null): string {
  if (!iso) return '—';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleTimeString();
}

/**
 * Relative-ish "last seen" stamp used on the Event types tab.
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
