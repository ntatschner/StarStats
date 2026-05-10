import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getCurrentLocation,
  getMyHangar,
  getMyProfile,
  getMyRsiOrgs,
  getSummary,
  getTimeline,
  getVehicleReferences,
  listEvents,
  type EventDto,
  type HangarSnapshot,
  type ProfileResponse,
  type ResolvedLocation,
  type RsiOrgsSnapshot,
  type SummaryResponse,
  type TimelineResponse,
  type VehicleListResponse,
} from '@/lib/api';
import { formatEventSummary } from '@/lib/event-summary';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';
import { LocationPill } from '@/components/LocationPill';
import { DayHeatmap } from '@/components/DayHeatmap';
import { HangarCard } from '@/components/HangarCard';
import { OrgsCard } from '@/components/OrgsCard';
import { ProfileCard } from '@/components/ProfileCard';

const PAGE_LIMIT = 50;

interface SearchParams {
  type?: string;
  before_seq?: string;
  after_seq?: string;
  since?: string;
  until?: string;
}

export default async function DashboardPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/dashboard');

  const params = await props.searchParams;
  const eventType = params.type;
  const beforeSeq = parseSeq(params.before_seq);
  const afterSeq = parseSeq(params.after_seq);
  const since = params.since;
  const until = params.until;

  const hasFilter =
    eventType !== undefined ||
    beforeSeq !== undefined ||
    afterSeq !== undefined ||
    since !== undefined ||
    until !== undefined;

  let summary: SummaryResponse;
  let recent: EventDto[];
  let timeline: TimelineResponse;
  let location: ResolvedLocation | null = null;
  try {
    const [summaryResp, eventsResp, timelineResp, locationResp] =
      await Promise.all([
        getSummary(session.token),
        listEvents(session.token, {
          limit: PAGE_LIMIT,
          event_type: eventType,
          before_seq: beforeSeq,
          after_seq: afterSeq,
          since,
          until,
        }),
        getTimeline(session.token, { days: 30 }),
        // Location resolver — 204 means "no recent activity", which
        // we treat as null and the pill renders nothing.
        getCurrentLocation(session.token).catch((e) => {
          // Don't redirect on 401 here — the parallel calls above
          // already do. Other errors degrade silently to "no pill".
          if (!(e instanceof ApiCallError) || e.status !== 401) {
            logger.warn({ err: e }, 'getCurrentLocation failed');
          }
          return null;
        }),
      ]);
    summary = summaryResp;
    recent = eventsResp.events;
    timeline = timelineResp;
    location = locationResp;
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/dashboard');
    }
    throw e;
  }

  // RSI citizen profile snapshot. 404 = "no snapshot yet" — render the
  // empty-state card prompting the user to refresh from settings. Any
  // other error degrades quietly (the card is opt-in scenery, not the
  // primary content of the page).
  let profile: ProfileResponse | null = null;
  try {
    profile = await getMyProfile(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/dashboard');
    }
    if (!(e instanceof ApiCallError) || e.status !== 404) {
      logger.warn({ err: e }, 'load my profile snapshot failed');
    }
  }

  // Hangar snapshot from the tray's most recent push. `getMyHangar`
  // already converts the server's 404 ("no_hangar_yet") into a typed
  // null, so the only thing left is the 401 redirect path.
  let hangar: HangarSnapshot | null = null;
  try {
    hangar = await getMyHangar(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/dashboard');
    }
    logger.warn({ err: e }, 'load hangar snapshot failed');
  }

  // RSI org memberships — server-scraped, refreshed manually from
  // /settings#rsi. Same null-on-404 pattern as hangar.
  let rsiOrgs: RsiOrgsSnapshot | null = null;
  try {
    rsiOrgs = await getMyRsiOrgs(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/dashboard');
    }
    logger.warn({ err: e }, 'load rsi orgs snapshot failed');
  }

  // Vehicle reference catalogue. Fetched separately from the primary
  // dashboard data so that a reference-API outage (or rate-limit hit)
  // degrades gracefully — the timeline simply renders raw class names
  // instead of friendly display names. Map is keyed by lowercased
  // class_name; `prettyVehicle` lowercases at lookup.
  let vehicleRefs: VehicleListResponse | null = null;
  try {
    vehicleRefs = await getVehicleReferences();
  } catch (e) {
    logger.warn({ err: e }, 'load vehicle references failed');
  }
  const vehicleNamesByClass: ReadonlyMap<string, string> | undefined = vehicleRefs
    ? new Map(
        vehicleRefs.vehicles.map((v) => [
          v.class_name.toLowerCase(),
          v.display_name,
        ]),
      )
    : undefined;

  const recentDesc = [...recent].sort((a, b) => b.seq - a.seq);
  const topTypes = [...summary.by_type]
    .sort((a, b) => b.count - a.count)
    .slice(0, 5);

  // Derived stat-strip values. None of these require new API calls —
  // we cherry-pick from the data already on the page so the strip is
  // an honest "here's what was just loaded" tile rather than a second
  // round-trip waiting to fail.
  const last30Total = timeline.buckets.reduce((acc, b) => acc + b.count, 0);
  const activeDays = timeline.buckets.reduce(
    (acc, b) => (b.count > 0 ? acc + 1 : acc),
    0,
  );
  const topTypeLabel = topTypes[0]?.event_type ?? '—';

  // Pager cursors. The API returns DESC by default for unparametrised
  // calls, so the largest seq is "newest" and the smallest is "oldest".
  const newestSeq = recentDesc.length ? recentDesc[0].seq : null;
  const oldestSeq = recentDesc.length
    ? recentDesc[recentDesc.length - 1].seq
    : null;

  const showOlder = recent.length === PAGE_LIMIT && oldestSeq !== null;
  // Show "Newer" whenever we're paging or filtering (i.e. there might
  // be more recent events beyond the current view).
  const showNewer = hasFilter && newestSeq !== null;

  const olderHref = oldestSeq !== null
    ? buildHref({ type: eventType, before_seq: oldestSeq })
    : null;
  const newerHref = newestSeq !== null
    ? buildHref({ type: eventType, after_seq: newestSeq })
    : null;
  const clearTypeHref = buildHref({}); // strips everything

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Manifest · last 30 days
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Hi,{' '}
          <span
            className="mono"
            style={{ color: 'var(--accent)', fontWeight: 500 }}
          >
            {session.claimedHandle}
          </span>
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
          }}
        >
          <span className="mono">{summary.total.toLocaleString()}</span>{' '}
          events captured across your hangar.
        </p>
      </header>

      <LocationPill location={location} />

      <ProfileCard profile={profile} showSettingsLink />

      <HangarCard snapshot={hangar} />

      <OrgsCard snapshot={rsiOrgs} showSettingsLink />

      {summary.total === 0 ? (
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
              Hangar
            </Link>{' '}
            page, launch Star Citizen, and play for a few minutes — events
            will start landing here as the tray drains them upstream.
          </p>
        </section>
      ) : (
        <>
          {/* Stat strip — derived from data already on the page. */}
          <div
            data-rsprow="nowrap"
            style={{
              display: 'flex',
              gap: 12,
              flexWrap: 'nowrap',
            }}
          >
            <StatTile
              eyebrow="Total events"
              value={summary.total.toLocaleString()}
              hint={`+${last30Total.toLocaleString()} last 30d`}
              hintKind="ok"
            />
            <StatTile
              eyebrow="Active days"
              value={String(activeDays)}
              hint={`of ${timeline.days} tracked`}
            />
            <StatTile
              eyebrow="Event types"
              value={String(summary.by_type.length)}
              hint="distinct signals"
            />
            <StatTile
              eyebrow="Top signal"
              value={topTypeLabel}
              hint={
                topTypes[0]
                  ? `${topTypes[0].count.toLocaleString()} fired`
                  : ''
              }
              mono
            />
          </div>

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
                  Activity
                </div>
                <h2
                  style={{
                    margin: 0,
                    fontSize: 17,
                    fontWeight: 600,
                    letterSpacing: '-0.01em',
                  }}
                >
                  Last {timeline.days} days
                </h2>
              </div>
            </header>
            <div style={{ padding: '16px 24px 22px' }}>
              <DayHeatmap timeline={timeline} />
            </div>
          </section>

          <div
            data-rspgrid="2"
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1.3fr',
              gap: 16,
            }}
          >
            <section className="ss-card">
              <header style={{ padding: '20px 24px 0' }}>
                <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
                  Distribution
                </div>
                <h2
                  style={{
                    margin: 0,
                    fontSize: 17,
                    fontWeight: 600,
                    letterSpacing: '-0.01em',
                  }}
                >
                  Top event types
                </h2>
              </header>
              <div style={{ padding: '16px 24px 22px' }}>
                <TypeBars
                  topTypes={topTypes}
                  total={summary.total}
                  buildHref={buildHref}
                />
              </div>
              <hr className="ss-rule" />
              <div
                style={{
                  padding: '14px 24px',
                  color: 'var(--fg-dim)',
                  fontSize: 12,
                }}
              >
                Click any signal to filter the stream.
              </div>
            </section>

            <section className="ss-card">
              <header style={{ padding: '20px 24px 0' }}>
                <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
                  Stream
                </div>
                <h2
                  style={{
                    margin: 0,
                    fontSize: 17,
                    fontWeight: 600,
                    letterSpacing: '-0.01em',
                  }}
                >
                  Recent activity
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
                      href={clearTypeHref}
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
                {recentDesc.length === 0 ? (
                  <p
                    style={{
                      margin: 0,
                      color: 'var(--fg-muted)',
                      fontSize: 13,
                    }}
                  >
                    Scope is clear. No events match this filter.
                  </p>
                ) : (
                  <Timeline
                    events={recentDesc}
                    vehicleNamesByClass={vehicleNamesByClass}
                  />
                )}
              </div>
              {(showOlder || showNewer) && (
                <>
                  <hr className="ss-rule" />
                  <nav
                    aria-label="Pagination"
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
          </div>
        </>
      )}
    </div>
  );
}

function parseSeq(raw: string | undefined): number | undefined {
  if (raw === undefined) return undefined;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? n : undefined;
}

function buildHref(opts: {
  type?: string;
  before_seq?: number;
  after_seq?: number;
}): Route {
  const qs = new URLSearchParams();
  if (opts.type) qs.set('type', opts.type);
  // Mutually exclusive — never include both cursors.
  if (opts.before_seq !== undefined) qs.set('before_seq', String(opts.before_seq));
  else if (opts.after_seq !== undefined) qs.set('after_seq', String(opts.after_seq));
  const suffix = qs.toString();
  // Cast to Route because the typed-routes RouteImpl<T> only accepts
  // template-literal-typed strings; URLSearchParams.toString() yields
  // plain `string`.
  return (suffix ? `/dashboard?${suffix}` : '/dashboard') as Route;
}

function formatTime(iso: string | null): string {
  if (!iso) return '—';
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return iso;
  return parsed.toLocaleTimeString();
}

/** Per-type accent colour mapping for the timeline border-left rail.
 * Mirrors the legacy `.timeline__item--*` rules but routed through
 * design tokens so themes still recolour cleanly. */
function eventBorderColor(eventType: string): string {
  switch (eventType) {
    case 'quantum_target_selected':
      return 'var(--accent)';
    case 'vehicle_stowed':
    case 'burst_summary':
      // Bursts collapse repetitive events into one summary row;
      // share the `--info` accent with `vehicle_stowed` since those
      // members are now folded into the summary in most cases.
      return 'var(--info)';
    case 'actor_death':
    case 'vehicle_destruction':
      return 'var(--danger)';
    case 'legacy_login':
    case 'join_pu':
    case 'mission_complete':
      return 'var(--ok)';
    default:
      return 'var(--border-strong)';
  }
}

/** Stat-strip tile. Mirrors design/prototype/app-screens.jsx Dashboard. */
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

/** Top-event-types ranked rows. 3-col grid: type label / fill bar / count. */
function TypeBars({
  topTypes,
  total,
  buildHref,
}: {
  topTypes: Array<{ event_type: string; count: number }>;
  total: number;
  buildHref: (opts: { type?: string }) => Route;
}) {
  return (
    <ul
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: 12,
      }}
    >
      {topTypes.map((t) => {
        const pct = total > 0 ? (t.count / total) * 100 : 0;
        return (
          <li
            key={t.event_type}
            style={{
              display: 'grid',
              gridTemplateColumns: 'minmax(0, 220px) 1fr 110px',
              gap: 14,
              alignItems: 'center',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            <Link
              href={buildHref({ type: t.event_type })}
              className="mono"
              style={{
                color: 'var(--accent)',
                textAlign: 'left',
                fontSize: 13,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                textDecoration: 'none',
              }}
            >
              {t.event_type}
            </Link>
            <span
              style={{
                display: 'block',
                height: 6,
                background: 'var(--grid-empty)',
                borderRadius: 3,
                overflow: 'hidden',
              }}
              aria-hidden="true"
            >
              <span
                style={{
                  display: 'block',
                  height: '100%',
                  width: `${pct}%`,
                  background: 'var(--accent)',
                  borderRadius: 3,
                }}
              />
            </span>
            <span
              style={{
                textAlign: 'right',
                fontSize: 13,
              }}
            >
              {t.count.toLocaleString()}
              <span style={{ color: 'var(--fg-dim)' }}>
                {' · '}
                {pct.toFixed(1)}%
              </span>
            </span>
          </li>
        );
      })}
    </ul>
  );
}

/** Recent activity timeline rendered as a 4-col grid:
 * time / source / summary / optional badge. The left border colour
 * encodes the event type (see `eventBorderColor`). */
function Timeline({
  events,
  vehicleNamesByClass,
}: {
  events: EventDto[];
  vehicleNamesByClass: ReadonlyMap<string, string> | undefined;
}) {
  return (
    <ol
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: 0,
        maxHeight: 520,
        overflowY: 'auto',
      }}
    >
      {events.map((e) => (
        <li
          key={e.seq}
          style={{
            display: 'grid',
            gridTemplateColumns: '78px 90px 1fr auto',
            gap: 14,
            alignItems: 'baseline',
            padding: '10px 12px',
            borderLeft: `2px solid ${eventBorderColor(e.event_type)}`,
            marginLeft: 4,
            fontSize: 13,
          }}
        >
          <time
            title={e.event_timestamp ?? ''}
            style={{
              color: 'var(--fg-dim)',
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
            }}
          >
            {formatTime(e.event_timestamp)}
          </time>
          <span
            className="mono"
            style={{
              color: 'var(--accent)',
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '0.05em',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {e.log_source}
          </span>
          <span style={{ color: 'var(--fg)', wordBreak: 'break-word' }}>
            {formatEventSummary(e.payload, vehicleNamesByClass)}
          </span>
        </li>
      ))}
    </ol>
  );
}
