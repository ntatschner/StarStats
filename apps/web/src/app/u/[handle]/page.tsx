/**
 * Public / friend profile view.
 *
 * Resolution order:
 *   1. Try `/v1/public/:handle/summary` — no token. If 200, render the
 *      profile as "public".
 *   2. If 404 and the visitor is logged in, retry against
 *      `/v1/u/:handle/summary` (the share-with-user path). If 200,
 *      render as "shared with you".
 *   3. Otherwise render a generic not-found message — never disclose
 *      whether the user exists.
 *
 * Scope: summary + top types + 30-day activity heatmap. The same
 * dual-mode resolution (`public` vs `shared`) drives which `/timeline`
 * endpoint we call.
 */

import Link from 'next/link';
import type { Route } from 'next';
import {
  ApiCallError,
  getFriendSummary,
  getFriendTimeline,
  getPublicProfile,
  getPublicRsiOrgs,
  getPublicSummary,
  getPublicTimeline,
  getSummary,
  getTimeline,
  type ProfileResponse,
  type PublicSummaryResponse,
  type PublicTimelineResponse,
  type RsiOrgsSnapshot,
} from '@/lib/api';
import { formatEventType } from '@/lib/event-types';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';
import { DayHeatmap } from '@/components/DayHeatmap';
import { OrgsCard } from '@/components/OrgsCard';
import { ProfileCard } from '@/components/ProfileCard';

interface PageProps {
  params: Promise<{ handle: string }>;
}

type View =
  | { kind: 'public'; data: PublicSummaryResponse }
  | { kind: 'shared'; data: PublicSummaryResponse }
  | { kind: 'self'; data: PublicSummaryResponse }
  | { kind: 'denied' };

async function resolveProfile(handle: string): Promise<View> {
  // 0. Self path — a signed-in user viewing their own /u/<handle>
  // would otherwise hit "denied" when their public-visibility toggle
  // is off. Short-circuit to their authenticated summary so the page
  // always works as a permalink to themselves. `claimedHandle` is
  // case-preserved from RSI so compare case-insensitively.
  const session = await getSession();
  if (
    session &&
    session.claimedHandle.toLowerCase() === handle.toLowerCase()
  ) {
    try {
      const data = await getSummary(session.token);
      // Structural cast — `SummaryResponse` and `PublicSummaryResponse`
      // share `by_type` / `claimed_handle` / `total`. Keeping the View
      // shape aligned avoids a second render path.
      return { kind: 'self', data: data as PublicSummaryResponse };
    } catch (e) {
      // 401 means the cookie outlived the server-side token; fall
      // through to the public/friend path so the user still sees
      // something useful instead of a hard error.
      if (!(e instanceof ApiCallError) || e.status !== 401) {
        logger.error({ err: e }, 'self summary fetch failed');
      }
    }
  }

  // 1. Public path — no auth.
  try {
    const data = await getPublicSummary(handle);
    return { kind: 'public', data };
  } catch (e) {
    if (!(e instanceof ApiCallError) || e.status !== 404) {
      // 503 (SpiceDB down) or any unexpected error — surface as denied
      // rather than crashing the route. Log so ops can see it.
      logger.error({ err: e }, 'public summary fetch failed');
      return { kind: 'denied' };
    }
  }

  // 2. Friend path — only if the visitor is logged in. Same 404 trap
  // applies: don't leak existence.
  if (!session) {
    return { kind: 'denied' };
  }
  try {
    const data = await getFriendSummary(session.token, handle);
    return { kind: 'shared', data };
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 404) {
      return { kind: 'denied' };
    }
    if (e instanceof ApiCallError && e.status === 401) {
      // Stale cookie — fall through to denied view rather than
      // bouncing through /auth/login. The user can navigate there
      // explicitly if they want to retry as themselves.
      return { kind: 'denied' };
    }
    logger.error({ err: e }, 'friend summary fetch failed');
    return { kind: 'denied' };
  }
}

export default async function PublicProfilePage(props: PageProps) {
  const { handle } = await props.params;
  const view = await resolveProfile(handle);

  if (view.kind === 'denied') {
    return (
      <div
        className="ss-screen-enter"
        style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
      >
        <header>
          <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
            Public profile
          </div>
          <h1
            style={{
              margin: 0,
              fontSize: 32,
              fontWeight: 600,
              letterSpacing: '-0.02em',
            }}
          >
            Profile not available
          </h1>
          <p
            style={{
              margin: '6px 0 0',
              color: 'var(--fg-muted)',
              fontSize: 14,
            }}
          >
            This profile either doesn&apos;t exist, isn&apos;t public, or
            hasn&apos;t been shared with you.
          </p>
        </header>
      </div>
    );
  }

  const { data } = view;
  const topTypes = [...data.by_type]
    .sort((a, b) => b.count - a.count)
    .slice(0, 5);

  // Fetch the citizen-profile snapshot for this handle. The endpoint
  // is unauthenticated but enforces public-or-shared visibility
  // server-side, so 404 here can mean "no snapshot yet", "not public",
  // or "not shared with you" — any of which collapse to "don't render
  // the card". Other failures degrade quietly: the rest of the page
  // is still useful.
  let profile: ProfileResponse | null = null;
  try {
    profile = await getPublicProfile(handle);
  } catch (e) {
    if (!(e instanceof ApiCallError) || e.status !== 404) {
      logger.warn({ err: e }, 'public profile snapshot fetch failed');
    }
  }

  // 30-day activity heatmap. Mirrors the tri-mode resolution above:
  // - self  -> authenticated `/me/timeline` (no visibility gate)
  // - public -> unauthenticated public endpoint
  // - shared -> friend endpoint (SpiceDB-gated)
  // 404/403 collapse to "no timeline" — same defensive posture as the
  // profile snapshot above.
  let timeline: PublicTimelineResponse | null = null;
  try {
    if (view.kind === 'self') {
      const session = await getSession();
      if (session) {
        // `TimelineResponse` shape matches `PublicTimelineResponse`
        // (same `buckets` + `days` fields) — cast keeps the heatmap
        // render path single-mode.
        const own = await getTimeline(session.token, { days: 30 });
        timeline = own as PublicTimelineResponse;
      }
    } else if (view.kind === 'public') {
      timeline = await getPublicTimeline(handle, 30);
    } else {
      const session = await getSession();
      if (session) {
        timeline = await getFriendTimeline(session.token, handle, 30);
      }
    }
  } catch (e) {
    if (!(e instanceof ApiCallError) || (e.status !== 404 && e.status !== 403)) {
      logger.warn({ err: e }, 'public/friend/self timeline fetch failed');
    }
  }

  // RSI org memberships — only the public endpoint is exposed for
  // strangers; there's no friend-scoped variant (sharing settings
  // gate the public route too). `getPublicRsiOrgs` already converts
  // 404/403 to null, so we just degrade quietly on any other error.
  let rsiOrgs: RsiOrgsSnapshot | null = null;
  try {
    rsiOrgs = await getPublicRsiOrgs(handle);
  } catch (e) {
    logger.warn({ err: e }, 'public rsi orgs fetch failed');
  }

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <header
        style={{
          display: 'flex',
          alignItems: 'flex-end',
          justifyContent: 'space-between',
          gap: 24,
          flexWrap: 'wrap',
        }}
      >
        <div style={{ flex: 1, minWidth: 240 }}>
          <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
            {view.kind === 'self'
              ? 'Your profile'
              : view.kind === 'public'
                ? 'Public profile'
                : 'Shared with you'}
          </div>
          <h1
            style={{
              margin: 0,
              fontSize: 36,
              fontWeight: 600,
              letterSpacing: '-0.02em',
            }}
          >
            <span className="mono">{data.claimed_handle}</span>
          </h1>
          <div
            style={{
              display: 'flex',
              gap: 10,
              flexWrap: 'wrap',
              marginTop: 10,
            }}
          >
            {view.kind === 'self' ? (
              <span className="ss-badge ss-badge--accent">
                <span className="ss-badge-dot" />
                You
              </span>
            ) : view.kind === 'public' ? (
              <span className="ss-badge ss-badge--accent">
                <span className="ss-badge-dot" />
                Public profile
              </span>
            ) : (
              <span className="ss-badge ss-badge--accent">
                Shared with you
              </span>
            )}
            {profile && (
              <span className="ss-badge ss-badge--ok">RSI verified</span>
            )}
          </div>
        </div>
        {/* Sharing CTAs — context-sensitive deep links into /sharing.
            Self view: jump to your own sharing management. Other
            users: pre-populate the add-handle field so the user can
            "share back" with a single confirm click. */}
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'flex-end',
            gap: 6,
          }}
        >
          {view.kind === 'self' ? (
            <Link
              href="/sharing"
              className="ss-btn ss-btn--ghost"
              style={{ textDecoration: 'none' }}
            >
              Manage sharing
            </Link>
          ) : (
            <Link
              href={
                (`/sharing?handle=${encodeURIComponent(data.claimed_handle)}`) as Route
              }
              className="ss-btn ss-btn--ghost"
              style={{ textDecoration: 'none' }}
            >
              Share back
            </Link>
          )}
        </div>
      </header>

      {/* Stat tiles. Public-safe: only the totals + top type, never
          the timeline windowed counts. */}
      <div
        data-rsprow="nowrap"
        style={{ display: 'flex', gap: 12, flexWrap: 'nowrap' }}
      >
        <PublicStatTile
          eyebrow="Total events"
          value={data.total.toLocaleString()}
        />
        <PublicStatTile
          eyebrow="Event types"
          value={String(data.by_type.length)}
        />
        <PublicStatTile
          eyebrow="Top signal"
          value={
            topTypes[0] ? formatEventType(topTypes[0].event_type).label : '—'
          }
        />
        <PublicStatTile
          eyebrow="Top count"
          value={
            topTypes[0] ? topTypes[0].count.toLocaleString() : '—'
          }
        />
      </div>

      {profile && <ProfileCard profile={profile} />}

      {rsiOrgs && <OrgsCard snapshot={rsiOrgs} />}

      {timeline && (
        <section className="ss-card">
          <header style={{ padding: '20px 24px 0' }}>
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
          </header>
          <div style={{ padding: '16px 24px 22px' }}>
            <DayHeatmap timeline={timeline} />
          </div>
        </section>
      )}

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
          {topTypes.length === 0 ? (
            <p
              style={{ margin: 0, color: 'var(--fg-muted)', fontSize: 13 }}
            >
              Scope is clear. No events recorded yet.
            </p>
          ) : (
            <PublicTypeBars topTypes={topTypes} total={data.total} />
          )}
        </div>
      </section>

      {view.kind === 'public' && (
        <div
          style={{
            padding: '14px 18px',
            background: 'var(--bg-elev)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--r-sm)',
            color: 'var(--fg-dim)',
            fontSize: 12,
            lineHeight: 1.5,
          }}
        >
          This is the public view — summary, top types, and a coarse
          activity heatmap only. The detailed timeline is only visible
          to handles or orgs the owner has explicitly shared with.
        </div>
      )}
      {view.kind === 'shared' && (
        <div
          style={{
            padding: '14px 18px',
            background: 'var(--bg-elev)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--r-sm)',
            color: 'var(--fg-dim)',
            fontSize: 12,
            lineHeight: 1.5,
          }}
        >
          <span className="mono" style={{ color: 'var(--fg)' }}>
            {data.claimed_handle}
          </span>{' '}
          has shared their manifest with you, so you see the full
          summary + timeline that public viewers don&apos;t.{' '}
          <Link
            href={
              (`/sharing?handle=${encodeURIComponent(data.claimed_handle)}`) as Route
            }
            style={{ color: 'var(--accent)' }}
          >
            Share back →
          </Link>
        </div>
      )}
    </div>
  );
}

/** Lightweight stat tile — public profile variant has no delta hint. */
function PublicStatTile({
  eyebrow,
  value,
}: {
  eyebrow: string;
  value: string;
}) {
  return (
    <div
      className="ss-card"
      style={{ flex: '1 1 200px', padding: '18px 20px', minWidth: 0 }}
    >
      <div className="ss-eyebrow">{eyebrow}</div>
      <div
        className="mono"
        style={{
          fontSize: 26,
          fontWeight: 500,
          letterSpacing: '-0.015em',
          margin: '8px 0 0',
          color: 'var(--fg)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {value}
      </div>
    </div>
  );
}

/** Top-event-types ranked rows — read-only variant (no filter links
 * because the public view doesn't expose a stream). */
function PublicTypeBars({
  topTypes,
  total,
}: {
  topTypes: Array<{ event_type: string; count: number }>;
  total: number;
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
            <span
              title={t.event_type}
              style={{
                color: 'var(--accent)',
                textAlign: 'left',
                fontSize: 13,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                display: 'inline-flex',
                gap: 6,
                alignItems: 'baseline',
              }}
            >
              <span aria-hidden="true">
                {formatEventType(t.event_type).glyph}
              </span>
              <span>{formatEventType(t.event_type).label}</span>
            </span>
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
            <span style={{ textAlign: 'right', fontSize: 13 }}>
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
