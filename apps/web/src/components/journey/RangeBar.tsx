/**
 * URL-synced time-range chip selector for journey stats tabs.
 *
 * The server-side stats endpoints all accept an `hours` parameter
 * (currently min 1, max 24*365 = 1y per `STATS_MAX_HOURS`). This
 * component renders a chip row of common windows and links each to
 * `/journey?view=<tab>&range=<id>`, preserving the active tab so
 * users don't lose their place when widening or narrowing the
 * timeframe.
 *
 * Server component — no client JS. URL is the source of truth.
 */

import Link from 'next/link';
import type { Route } from 'next';

/** Canonical range definitions. `id` is the URL token; `hours`
 *  feeds the stats endpoint; `label` is what the user sees. */
const RANGES = [
  { id: '24h', label: '24h', hours: 24 },
  { id: '7d', label: '7d', hours: 24 * 7 },
  { id: '30d', label: '30d', hours: 24 * 30 },
  { id: '90d', label: '90d', hours: 24 * 90 },
  { id: 'all', label: 'All', hours: 24 * 365 },
] as const;

export type RangeId = (typeof RANGES)[number]['id'];

const RANGE_IDS = RANGES.map((r) => r.id) as readonly string[];

/** Parse a `?range=` value into a known id, falling back to the
 *  default (`30d`) for anything missing or unrecognised. The default
 *  matches the server-side `STATS_DEFAULT_HOURS` constant so a URL
 *  with no range param renders the same window the server would
 *  pick on its own. */
export function parseRange(raw: string | undefined): RangeId {
  if (raw && RANGE_IDS.includes(raw)) return raw as RangeId;
  return '30d';
}

export function rangeToHours(id: RangeId): number {
  return RANGES.find((r) => r.id === id)!.hours;
}

export function RangeBar({
  active,
  view,
}: {
  active: RangeId;
  /** Current journey tab — preserved in the chip links so the
   *  user stays where they are. */
  view: string;
}) {
  return (
    <nav
      aria-label="Time range"
      style={{
        display: 'flex',
        gap: 4,
        flexWrap: 'wrap',
        alignItems: 'center',
      }}
    >
      <span
        className="ss-eyebrow"
        style={{ marginRight: 6, color: 'var(--fg-dim)' }}
      >
        Range
      </span>
      {RANGES.map((r) => {
        const isActive = r.id === active;
        const href = (
          view === 'location' || view === 'commerce'
            ? `/journey?range=${r.id}`
            : `/journey?view=${view}&range=${r.id}`
        ) as Route;
        return (
          <Link
            key={r.id}
            href={href}
            aria-current={isActive ? 'page' : undefined}
            className="mono"
            style={{
              padding: '4px 10px',
              fontSize: 12,
              borderRadius: 4,
              textDecoration: 'none',
              color: isActive ? 'var(--bg)' : 'var(--fg-muted)',
              background: isActive ? 'var(--accent)' : 'transparent',
              border: `1px solid ${isActive ? 'var(--accent)' : 'var(--border)'}`,
              letterSpacing: '0.02em',
            }}
          >
            {r.label}
          </Link>
        );
      })}
    </nav>
  );
}
