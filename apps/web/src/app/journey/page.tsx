/**
 * /journey — activity surface.
 *
 * Internal tabs, each fetched conditionally based on the active URL
 * param:
 *   - Location (default): current pill + recent journey trace
 *   - Travel: top quantum destinations + planets visited
 *   - Combat: top weapons + deaths by zone
 *   - Loadout: top attached items
 *   - Stability: crashes per channel
 *   - Types: event-type breakdown + per-type raw stream (merged in
 *     from the deprecated `/metrics` page per audit v2 §07)
 *
 * Server component. Each tab fetch is conditional so a user looking
 * at `Travel` doesn't pay for the Combat / Loadout / Stability calls.
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getCombatStats,
  getCommerceRecent,
  getCurrentLocation,
  getLoadoutStats,
  getLocationBreakdown,
  getLocationTrace,
  getStabilityStats,
  getTravelStats,
  type BreakdownResponse,
  type CombatStatsResponse,
  type CommerceRecentResponse,
  type CommerceTransaction,
  type CommerceTxStatus,
  type LoadoutStatsResponse,
  type ResolvedLocation,
  type StabilityStatsResponse,
  type TraceResponse,
  type TravelStatsResponse,
} from '@/lib/api';
import { LocationPill } from '@/components/LocationPill';
import {
  HierarchicalBucketList,
  rollUpItems,
  rollUpLocations,
  rollUpWeapons,
  type RollupNode,
} from '@/components/journey/HierarchicalBucketList';
import { LocationChainStrip } from '@/components/journey/LocationChainStrip';
import { LocationConstellation } from '@/components/journey/LocationConstellation';
import { LocationFrequencyBars } from '@/components/journey/LocationFrequencyBars';
import { LocationTimeline } from '@/components/journey/LocationTimeline';
import {
  parseRange,
  rangeToHours,
  RangeBar,
  type RangeId,
} from '@/components/journey/RangeBar';
import { logger } from '@/lib/logger';
import {
  getLocationCatalog,
  getReferences,
  prettyClass,
} from '@/lib/reference';
import { getSession } from '@/lib/session';
import { parseMetricsRange, TypesTab } from './_components/TypesTab';

const TAB_IDS = [
  'location',
  'travel',
  'combat',
  'loadout',
  'stability',
  'commerce',
  'types',
] as const;
type TabId = (typeof TAB_IDS)[number];

const TAB_LABELS: Record<TabId, string> = {
  location: 'Location',
  travel: 'Travel',
  combat: 'Combat',
  loadout: 'Loadout',
  stability: 'Stability',
  commerce: 'Commerce',
  types: 'Types',
};

interface SearchParams {
  view?: string;
  range?: string;
  // Types tab — preserves the legacy `/metrics?view=raw&type=…`
  // contract that the global TopBar search still posts. `type` pins
  // the filter; `before_seq` / `after_seq` drive cursor pagination.
  type?: string;
  before_seq?: string;
  after_seq?: string;
}

function parseTab(raw?: string): TabId {
  if (raw && (TAB_IDS as readonly string[]).includes(raw)) {
    return raw as TabId;
  }
  return 'location';
}

export default async function JourneyPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/journey');

  const params = await props.searchParams;
  const view = parseTab(params.view);
  const range = parseRange(params.range);
  const hours = rangeToHours(range);

  // Stats tabs aggregate by raw `class_name` server-side, so we
  // humanize the bucket labels client-side. Each tab loads only the
  // catalog categories it needs, in parallel with its stats fetch —
  // see TravelTab / CombatTab / LoadoutTab below. The page-level
  // `loadAllReferences()` we used to call here blocked the whole
  // render on the 20K-entry items map even when the active tab
  // didn't need it; cutting it removes the worst sequential wait.
  //
  // Location uses fixed sub-windows (24h / 7d) per its UX titles —
  // the chip row would be misleading there. Types owns its own
  // metrics-range switcher (7d/30d/90d/all) inside the breakdown
  // card. Every other tab honors the page-level hour-based range.
  // The server's commerce endpoint now accepts hours too (paired in
  // the same commit), so the chip strip works there.
  const showRangeBar = view !== 'location' && view !== 'types';

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20, maxWidth: 960 }}
    >
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Activity · refined signal from your captures
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Journey
        </h1>
        <p style={{ margin: '6px 0 0', color: 'var(--fg-muted)', fontSize: 14 }}>
          Where you go, what you fight, and what you carry — pulled from the
          events your tray has captured.
        </p>
      </header>

      <Tabs active={view} range={range} />

      {showRangeBar && (
        <RangeBar
          active={range}
          // `showRangeBar` already excludes the location tab, so
          // `view` is always one of the stats/commerce tabs here.
          buildHref={(id) =>
            `/journey?view=${view}&range=${id}` as Route
          }
        />
      )}

      {view === 'location' && (
        <LocationTab token={session.token} />
      )}
      {view === 'travel' && (
        <TravelTab token={session.token} hours={hours} />
      )}
      {view === 'combat' && (
        <CombatTab token={session.token} hours={hours} />
      )}
      {view === 'loadout' && (
        <LoadoutTab token={session.token} hours={hours} />
      )}
      {view === 'stability' && (
        <StabilityTab token={session.token} hours={hours} />
      )}
      {view === 'commerce' && (
        <CommerceTab token={session.token} hours={hours} />
      )}
      {view === 'types' && (
        <TypesTab
          token={session.token}
          range={parseMetricsRange(params.range)}
          eventType={params.type}
          beforeSeqRaw={params.before_seq}
          afterSeqRaw={params.after_seq}
        />
      )}
    </div>
  );
}

function Tabs({ active, range }: { active: TabId; range: RangeId }) {
  // Carry the current range across tab switches so users don't lose
  // their selected timeframe when navigating between Combat/Travel/
  // Loadout. Default range (30d) is omitted from the URL to keep
  // shared links short.
  const rangeQuery = range === '30d' ? '' : `range=${range}`;
  return (
    <nav
      style={{
        display: 'flex',
        gap: 4,
        flexWrap: 'wrap',
        borderBottom: '1px solid var(--border)',
      }}
    >
      {TAB_IDS.map((id) => {
        const isActive = id === active;
        const params: string[] = [];
        if (id !== 'location') params.push(`view=${id}`);
        if (rangeQuery) params.push(rangeQuery);
        const href = (
          params.length === 0 ? '/journey' : `/journey?${params.join('&')}`
        ) as Route;
        return (
          <Link
            key={id}
            href={href}
            style={{
              padding: '10px 14px',
              fontSize: 13,
              fontWeight: 500,
              color: isActive ? 'var(--fg)' : 'var(--fg-muted)',
              borderBottom: isActive
                ? '2px solid var(--accent)'
                : '2px solid transparent',
              textDecoration: 'none',
              marginBottom: -1,
            }}
          >
            {TAB_LABELS[id]}
          </Link>
        );
      })}
    </nav>
  );
}

// -- Location tab --------------------------------------------------

async function LocationTab({ token }: { token: string }) {
  let current: ResolvedLocation | null = null;
  let trace: TraceResponse | null = null;
  let breakdown: BreakdownResponse | null = null;
  try {
    const [c, t, b] = await Promise.all([
      getCurrentLocation(token).catch(() => null),
      getLocationTrace(token, 24).catch((e) => {
        logger.warn({ err: e }, 'trace fetch failed');
        return null;
      }),
      getLocationBreakdown(token, 24 * 7).catch((e) => {
        logger.warn({ err: e }, 'breakdown fetch failed');
        return null;
      }),
    ]);
    current = c;
    trace = t;
    breakdown = b;
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey');
    }
    throw e;
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <LocationPill location={current} />

      {trace && trace.entries.length > 0 && (
        <LocationChainStrip
          entries={trace.entries}
          maxStops={5}
          eyebrow="Recent stops · last 24h"
        />
      )}

      <section className="ss-card" style={{ padding: '18px 20px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Constellation · last 24h
        </div>
        {trace && trace.entries.length > 0 ? (
          <LocationConstellation entries={trace.entries} />
        ) : (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            No stops to chart yet.
          </p>
        )}
      </section>

      <section className="ss-card" style={{ padding: '18px 20px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Recent journey · last 24h
        </div>
        {trace && trace.entries.length > 0 ? (
          <LocationTimeline entries={trace.entries} />
        ) : (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            No location-bearing events in the last 24 hours.
          </p>
        )}
      </section>

      <section className="ss-card" style={{ padding: '18px 20px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Where you spend time · last 7 days
        </div>
        <LocationFrequencyBars entries={breakdown?.entries ?? []} />
      </section>
    </div>
  );
}

// -- Travel tab ----------------------------------------------------

async function TravelTab({
  token,
  hours,
}: {
  token: string;
  hours: number;
}) {
  let stats: TravelStatsResponse;
  let locations;
  try {
    // Run stats + locations catalog in parallel so the slower of
    // the two governs total wait, not the sum. Travel only needs
    // the locations category — vehicles/weapons/items would be
    // wasted bytes on this tab. The catalog carries wiki hierarchy
    // metadata (system/parent) so the rollup can resolve every
    // wiki-known location without hardcoded dictionaries.
    [stats, locations] = await Promise.all([
      getTravelStats(token, hours),
      getLocationCatalog(),
    ]);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey?view=travel');
    }
    throw e;
  }
  return (
    <StatsLayout
      headline={{ label: 'Quantum jumps', value: stats.quantum_jumps }}
      blocks={[
        {
          title: 'Top destinations',
          tree: rollUpLocations(stats.top_destinations, locations),
        },
        {
          title: 'Planets visited',
          tree: rollUpLocations(stats.planets_visited, locations),
        },
      ]}
      hours={stats.hours}
    />
  );
}

// -- Combat tab ----------------------------------------------------

async function CombatTab({
  token,
  hours,
}: {
  token: string;
  hours: number;
}) {
  let stats: CombatStatsResponse;
  let weapons;
  let locations;
  try {
    // Stats + weapons + locations in parallel. Combat doesn't need
    // vehicles or items, so we skip those two big maps entirely.
    // Locations use the rich catalog (wiki metadata for hierarchy);
    // weapons stay on the display-only map for now (Wave 2 will
    // upgrade them).
    [stats, weapons, locations] = await Promise.all([
      getCombatStats(token, hours),
      getReferences('weapon'),
      getLocationCatalog(),
    ]);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey?view=combat');
    }
    throw e;
  }
  // K/D ratio surface — only meaningful when there have been deaths.
  // A perfect run (deaths=0, kills>0) shows "—" rather than ∞ to
  // avoid implying a divide-by-zero. We compute as a string up front
  // so the JSX stays clean.
  const kdRatio =
    stats.deaths === 0
      ? stats.kills === 0
        ? '—'
        : '∞'
      : (stats.kills / stats.deaths).toFixed(2);

  // Design audit v2 §07: hide the K/D tile when the window is empty
  // on both sides — "0 / 0 / —" is noise. A "Scope is clear" empty
  // state matches the in-universe voice used elsewhere.
  const hasCombatActivity = stats.kills > 0 || stats.deaths > 0;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      {hasCombatActivity ? (
        <section
          className="ss-card"
          style={{
            padding: '18px 20px',
            display: 'grid',
            gridTemplateColumns: 'repeat(3, 1fr)',
            gap: 16,
          }}
        >
          <KdTile label="Kills" value={stats.kills} tone="var(--ok)" />
          <KdTile label="Deaths" value={stats.deaths} tone="var(--err)" />
          <KdTile label="K/D" value={kdRatio} tone="var(--accent)" />
        </section>
      ) : (
        <section className="ss-card" style={{ padding: '18px 20px' }}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Combat
          </div>
          <p style={{ margin: 0, color: 'var(--fg-muted)', fontSize: 13 }}>
            Scope is clear. No kills or deaths in this window.
          </p>
        </section>
      )}
      <p style={{ margin: 0, fontSize: 11, color: 'var(--fg-dim)' }}>
        Past {formatHoursWindow(stats.hours)}.
      </p>
      <section className="ss-card" style={{ padding: '18px 20px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Top weapons (your kills)
        </div>
        {stats.top_weapons.length === 0 ? (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            No kills recorded in this window.
          </p>
        ) : (
          <HierarchicalBucketList
            nodes={rollUpWeapons(stats.top_weapons, weapons)}
          />
        )}
      </section>
      <section className="ss-card" style={{ padding: '18px 20px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Hot zones (where you die)
        </div>
        {stats.deaths_by_zone.length === 0 ? (
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
            No deaths recorded in this window.
          </p>
        ) : (
          <HierarchicalBucketList
            nodes={rollUpLocations(stats.deaths_by_zone, locations)}
          />
        )}
      </section>
    </div>
  );
}

function KdTile({
  label,
  value,
  tone,
}: {
  label: string;
  value: number | string;
  tone: string;
}) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column' }}>
      <span style={{ fontSize: 11, color: 'var(--fg-dim)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
        {label}
      </span>
      <span
        className="mono"
        style={{
          fontSize: 32,
          fontWeight: 700,
          color: tone,
          letterSpacing: '-0.02em',
        }}
      >
        {typeof value === 'number' ? value.toLocaleString() : value}
      </span>
    </div>
  );
}

// -- Loadout tab ---------------------------------------------------

async function LoadoutTab({
  token,
  hours,
}: {
  token: string;
  hours: number;
}) {
  let stats: LoadoutStatsResponse;
  let items;
  try {
    // Loadout is the only stats tab that genuinely needs the items
    // category (~20K entries). Still worth parallelising with stats
    // so the latencies overlap.
    [stats, items] = await Promise.all([
      getLoadoutStats(token, hours),
      getReferences('item'),
    ]);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey?view=loadout');
    }
    throw e;
  }
  return (
    <StatsLayout
      headline={{ label: 'Items attached', value: stats.attachments }}
      blocks={[
        {
          title: 'Most-attached items',
          tree: rollUpItems(stats.top_items, items),
        },
      ]}
      hours={stats.hours}
    />
  );
}

// -- Stability tab -------------------------------------------------

async function StabilityTab({
  token,
  hours,
}: {
  token: string;
  hours: number;
}) {
  let stats: StabilityStatsResponse;
  try {
    stats = await getStabilityStats(token, hours);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey?view=stability');
    }
    throw e;
  }
  return (
    <StatsLayout
      headline={{ label: 'Crashes', value: stats.crashes }}
      blocks={[{ title: 'Crashes by channel', buckets: stats.by_channel }]}
      hours={stats.hours}
    />
  );
}

// -- Shared layout for stats tabs ---------------------------------

/** Two block shapes feed `StatsLayout`:
 *   - `tree` — pre-rolled-up hierarchical buckets (preferred for
 *     class-name dimensions: weapons / items / locations).
 *   - `buckets` (+ optional `format`) — flat rendering, used for
 *     already-readable strings like the stability `by_channel`
 *     log-channel names. */
type StatsBlock =
  | {
      title: string;
      tree: RollupNode[];
    }
  | {
      title: string;
      buckets: { value: string; count: number }[];
      format?: (raw: string) => string;
    };

function StatsLayout({
  headline,
  blocks,
  hours,
  caveat,
}: {
  headline: { label: string; value: number };
  blocks: StatsBlock[];
  hours: number;
  caveat?: string;
}) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <section
        className="ss-card"
        style={{
          padding: '18px 20px',
          display: 'flex',
          alignItems: 'baseline',
          gap: 16,
        }}
      >
        <span
          className="mono"
          style={{
            fontSize: 36,
            fontWeight: 700,
            letterSpacing: '-0.02em',
            color: 'var(--accent)',
          }}
        >
          {headline.value.toLocaleString()}
        </span>
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <span style={{ fontSize: 14, color: 'var(--fg)' }}>
            {headline.label}
          </span>
          <span style={{ fontSize: 11, color: 'var(--fg-dim)' }}>
            past {formatHoursWindow(hours)}
          </span>
        </div>
      </section>
      {blocks.map((block) => (
        <section
          key={block.title}
          className="ss-card"
          style={{ padding: '18px 20px' }}
        >
          <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
            {block.title}
          </div>
          {'tree' in block
            ? block.tree.length === 0
              ? (
                <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
                  No data yet.
                </p>
              )
              : <HierarchicalBucketList nodes={block.tree} />
            : block.buckets.length === 0
              ? (
                <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
                  No data yet.
                </p>
              )
              : <BucketList buckets={block.buckets} format={block.format} />}
        </section>
      ))}
      {caveat && (
        <p
          style={{
            margin: 0,
            fontSize: 11,
            color: 'var(--fg-dim)',
            fontStyle: 'italic',
          }}
        >
          {caveat}
        </p>
      )}
    </div>
  );
}

function BucketList({
  buckets,
  format,
}: {
  buckets: { value: string; count: number }[];
  format?: (raw: string) => string;
}) {
  const max = Math.max(...buckets.map((b) => b.count), 1);
  return (
    <ol
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      {buckets.map((b) => {
        const pct = (b.count / max) * 100;
        const label = format ? format(b.value) : b.value;
        return (
          <li key={b.value}>
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                fontSize: 12,
                marginBottom: 3,
              }}
            >
              <span
                className="mono"
                style={{ color: 'var(--fg)', overflow: 'hidden' }}
                title={b.value !== label ? b.value : undefined}
              >
                {label}
              </span>
              <span
                className="mono"
                style={{ color: 'var(--fg-dim)' }}
              >
                {b.count.toLocaleString()}
              </span>
            </div>
            <div
              style={{
                height: 4,
                background: 'var(--bg-elev)',
                borderRadius: 2,
                overflow: 'hidden',
              }}
            >
              <div
                style={{
                  width: `${pct}%`,
                  height: '100%',
                  background: 'var(--accent)',
                }}
              />
            </div>
          </li>
        );
      })}
    </ol>
  );
}

// -- Formatters ----------------------------------------------------

function formatHoursWindow(hours: number): string {
  if (hours <= 24) return `${hours}h`;
  const days = Math.round(hours / 24);
  return `${days}d`;
}

function formatTimeShort(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}


// -- Commerce tab --------------------------------------------------

async function CommerceTab({
  token,
  hours,
}: {
  token: string;
  hours: number;
}) {
  let response: CommerceRecentResponse | null = null;
  try {
    response = await getCommerceRecent(token, 100, 30, hours);
  } catch (e) {
    if (e instanceof ApiCallError) {
      logger.warn(
        { status: e.status, body: e.body },
        'getCommerceRecent failed',
      );
    } else {
      logger.warn({ err: String(e) }, 'getCommerceRecent threw');
    }
  }

  const txs = response?.transactions ?? [];

  if (txs.length === 0) {
    return (
      <div
        style={{
          padding: '32px 16px',
          color: 'var(--fg-muted)',
          fontSize: 14,
          textAlign: 'center',
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 8,
        }}
      >
        No shop or commodity transactions captured yet. Make a purchase
        in-game and re-sync to see them here.
      </div>
    );
  }

  return (
    <section style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: '110px 1fr 130px 110px',
          gap: 16,
          padding: '8px 14px',
          fontSize: 12,
          color: 'var(--fg-muted)',
          fontWeight: 500,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          borderBottom: '1px solid var(--border)',
        }}
      >
        <div>Kind</div>
        <div>Item</div>
        <div>Time</div>
        <div style={{ textAlign: 'right' }}>Status</div>
      </div>
      <ol style={{ listStyle: 'none', margin: 0, padding: 0 }}>
        {txs.map((tx, idx) => (
          <li
            key={`${tx.started_at}-${idx}`}
            style={{
              display: 'grid',
              gridTemplateColumns: '110px 1fr 130px 110px',
              gap: 16,
              padding: '10px 14px',
              borderBottom: '1px solid var(--border)',
              fontSize: 14,
              alignItems: 'baseline',
            }}
          >
            <span style={{ color: 'var(--fg-muted)' }}>
              {formatCommerceKind(tx)}
            </span>
            <span>
              {tx.item ?? '—'}
              {tx.quantity != null ? ` × ${tx.quantity}` : ''}
            </span>
            <span style={{ color: 'var(--fg-muted)', fontVariantNumeric: 'tabular-nums' }}>
              {formatTimeShort(tx.started_at)}
            </span>
            <span
              style={{
                color: commerceStatusColor(tx.status),
                fontWeight: 500,
                textAlign: 'right',
              }}
            >
              {formatCommerceStatus(tx.status)}
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}

function formatCommerceKind(tx: CommerceTransaction): string {
  switch (tx.kind) {
    case 'shop':
      return 'Shop';
    case 'commodity_buy':
      return 'Commodity buy';
    case 'commodity_sell':
      return 'Commodity sell';
  }
}

function formatCommerceStatus(s: CommerceTxStatus): string {
  switch (s) {
    case 'pending':
      return 'Pending';
    case 'confirmed':
      return 'Confirmed';
    case 'rejected':
      return 'Rejected';
    case 'timed_out':
      return 'Timed out';
    case 'submitted':
      return 'Submitted';
  }
}

function commerceStatusColor(s: CommerceTxStatus): string {
  switch (s) {
    case 'confirmed':
      return 'var(--success, #4ade80)';
    case 'rejected':
    case 'timed_out':
      return 'var(--error, #f87171)';
    case 'pending':
      return 'var(--warning, #fbbf24)';
    case 'submitted':
      return 'var(--accent, #60a5fa)';
  }
}
