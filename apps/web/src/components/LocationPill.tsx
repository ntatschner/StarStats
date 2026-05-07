/**
 * "You are here" pill — surfaces the most recent location reading
 * from `GET /v1/me/location/current`. Renders nothing when the
 * server returns 204 (no recent activity).
 *
 * Server-component shape: pass the resolved location (or null) in
 * via props. The fetch happens upstream where the bearer token
 * lives. Keeping the component pure makes it composable across
 * /dashboard, /metrics, and the /journey page without each one
 * needing its own fetcher.
 */

import type { ResolvedLocation } from '@/lib/api';

export function LocationPill({
  location,
}: {
  location: ResolvedLocation | null;
}) {
  if (location === null) {
    return null;
  }

  // Build the display label from the most precise field available.
  // City > planet > shard. The pill stays informative even when only
  // partial data is available (e.g. JoinPu with no follow-up).
  const headline =
    location.city ??
    location.planet ??
    location.shard ??
    'In transit';
  const subline = buildSubline(location);
  const since = formatAge(location.last_seen_at);

  return (
    <section
      className="ss-card"
      style={{
        padding: '14px 18px',
        display: 'flex',
        alignItems: 'center',
        gap: 16,
        borderColor: 'var(--accent)',
      }}
      aria-label="Current in-game location"
    >
      <span aria-hidden style={{ fontSize: 22, lineHeight: 1 }}>
        {pickGlyph(location)}
      </span>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, flex: 1 }}>
        <span
          style={{
            fontSize: 11,
            color: 'var(--fg-dim)',
            textTransform: 'uppercase',
            letterSpacing: '0.06em',
          }}
        >
          You are here
        </span>
        <span
          className="mono"
          style={{
            fontSize: 16,
            fontWeight: 600,
            letterSpacing: '-0.01em',
            color: 'var(--fg)',
          }}
        >
          {headline}
        </span>
        {subline && (
          <span
            style={{
              fontSize: 12,
              color: 'var(--fg-muted)',
            }}
          >
            {subline}
          </span>
        )}
      </div>
      <span
        className="mono"
        style={{
          fontSize: 11,
          color: 'var(--fg-dim)',
        }}
        title={location.last_seen_at}
      >
        {since}
      </span>
    </section>
  );
}

function buildSubline(loc: ResolvedLocation): string | null {
  // When the headline is a city we add the planet + system; when it's
  // a planet we add the system; when it's a shard-only fallback we
  // skip the subline.
  const parts: string[] = [];
  if (loc.city && loc.planet) {
    parts.push(loc.planet);
  }
  if (loc.system && loc.system !== loc.planet) {
    parts.push(loc.system);
  }
  if (loc.shard && parts.length === 0) {
    parts.push(`Shard ${loc.shard}`);
  } else if (loc.shard) {
    parts.push(`shard ${loc.shard}`);
  }
  return parts.length > 0 ? parts.join(' · ') : null;
}

function pickGlyph(loc: ResolvedLocation): string {
  // Pure cosmetic — distinguishes the three resolution sources at a
  // glance. No icon font dependency; a Unicode glyph is enough.
  if (loc.city) return '🛰';
  if (loc.planet) return '🪐';
  return '✦';
}

function formatAge(isoTimestamp: string): string {
  const ts = new Date(isoTimestamp);
  if (Number.isNaN(ts.getTime())) return '';
  const ageMs = Date.now() - ts.getTime();
  const ageMin = Math.floor(ageMs / 60_000);
  if (ageMin < 1) return 'just now';
  if (ageMin < 60) return `${ageMin}m ago`;
  const ageHr = Math.floor(ageMin / 60);
  if (ageHr < 24) return `${ageHr}h ago`;
  const ageDay = Math.floor(ageHr / 24);
  return `${ageDay}d ago`;
}
