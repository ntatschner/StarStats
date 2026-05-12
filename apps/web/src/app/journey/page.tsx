/**
 * /journey — activity surface.
 *
 * Five internal tabs, each fetched conditionally based on the active
 * URL param:
 *   - Location (default): current pill + recent journey trace
 *   - Travel: top quantum destinations + planets visited
 *   - Combat: top weapons + deaths by zone
 *   - Loadout: top attached items
 *   - Stability: crashes per channel
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
import { LocationChainStrip } from '@/components/journey/LocationChainStrip';
import { LocationConstellation } from '@/components/journey/LocationConstellation';
import { LocationFrequencyBars } from '@/components/journey/LocationFrequencyBars';
import { LocationTimeline } from '@/components/journey/LocationTimeline';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';

const TAB_IDS = [
  'location',
  'travel',
  'combat',
  'loadout',
  'stability',
  'commerce',
] as const;
type TabId = (typeof TAB_IDS)[number];

const TAB_LABELS: Record<TabId, string> = {
  location: 'Location',
  travel: 'Travel',
  combat: 'Combat',
  loadout: 'Loadout',
  stability: 'Stability',
  commerce: 'Commerce',
};

interface SearchParams {
  view?: string;
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

      <Tabs active={view} />

      {view === 'location' && (
        <LocationTab token={session.token} />
      )}
      {view === 'travel' && <TravelTab token={session.token} />}
      {view === 'combat' && <CombatTab token={session.token} />}
      {view === 'loadout' && <LoadoutTab token={session.token} />}
      {view === 'stability' && <StabilityTab token={session.token} />}
      {view === 'commerce' && <CommerceTab token={session.token} />}
    </div>
  );
}

function Tabs({ active }: { active: TabId }) {
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
        const href = (id === 'location'
          ? '/journey'
          : `/journey?view=${id}`) as Route;
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

async function TravelTab({ token }: { token: string }) {
  let stats: TravelStatsResponse;
  try {
    stats = await getTravelStats(token);
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
        { title: 'Top destinations', buckets: stats.top_destinations },
        { title: 'Planets visited', buckets: stats.planets_visited },
      ]}
      hours={stats.hours}
    />
  );
}

// -- Combat tab ----------------------------------------------------

async function CombatTab({ token }: { token: string }) {
  let stats: CombatStatsResponse;
  try {
    stats = await getCombatStats(token);
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

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
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
          <BucketList buckets={stats.top_weapons} />
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
          <BucketList buckets={stats.deaths_by_zone} />
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

async function LoadoutTab({ token }: { token: string }) {
  let stats: LoadoutStatsResponse;
  try {
    stats = await getLoadoutStats(token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/journey?view=loadout');
    }
    throw e;
  }
  return (
    <StatsLayout
      headline={{ label: 'Items attached', value: stats.attachments }}
      blocks={[{ title: 'Most-attached items', buckets: stats.top_items }]}
      hours={stats.hours}
    />
  );
}

// -- Stability tab -------------------------------------------------

async function StabilityTab({ token }: { token: string }) {
  let stats: StabilityStatsResponse;
  try {
    stats = await getStabilityStats(token);
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

interface StatsBlock {
  title: string;
  buckets: { value: string; count: number }[];
}

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
          {block.buckets.length === 0 ? (
            <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
              No data yet.
            </p>
          ) : (
            <BucketList buckets={block.buckets} />
          )}
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
}: {
  buckets: { value: string; count: number }[];
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
              >
                {b.value}
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

async function CommerceTab({ token }: { token: string }) {
  let response: CommerceRecentResponse | null = null;
  try {
    response = await getCommerceRecent(token, 100, 30);
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
