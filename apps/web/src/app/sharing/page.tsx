/**
 * Sharing — outbound + inbound stats-record sharing surface.
 *
 * Promoted from a buried section inside /settings#sharing into its
 * own surface so:
 *  - users can discover sharing without spelunking through settings,
 *  - the inbound side ("who has shared with you") has a home, and
 *  - per-profile CTAs ("Manage sharing", "Share back") can deep-link
 *    to a single canonical place rather than a settings anchor.
 *
 * Backend contracts:
 *  - GET  /v1/me/visibility                — public toggle state
 *  - POST /v1/me/visibility {public:bool}  — flip the toggle
 *  - GET  /v1/me/shares                    — outbound (user + org)
 *  - POST /v1/me/share {recipient_handle}  — grant to handle
 *  - POST /v1/me/share/org {org_slug}      — grant to org
 *  - DEL  /v1/me/share/:recipient_handle   — revoke handle
 *  - DEL  /v1/me/share/org/:slug           — revoke org
 *  - GET  /v1/me/shared-with-me            — inbound (new in this wave)
 *
 * SpiceDB unavailability degrades the UI but never blocks the page —
 * the user can still navigate away. RSI-unverified callers can read
 * state (degraded mode warning) but mutation handlers return 403.
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  addShare,
  getVisibility,
  listOrgs,
  listShares,
  listSharedWithMe,
  removeShare,
  setVisibility,
  shareWithOrg,
  unshareWithOrg,
  type ListOrgsResponse,
  type ListSharedWithMeResponse,
  type ListSharesResponse,
  type VisibilityResponse,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';

interface SearchParams {
  status?: string;
  error?: string;
  /** Pre-populate the add-handle field — set by per-profile "Share back" CTA. */
  handle?: string;
}

const pageStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 20,
  maxWidth: 960,
  margin: '0 auto',
  padding: '8px 0 60px',
};

const cardHeaderStyle: React.CSSProperties = { padding: '20px 24px 0' };
const cardBodyStyle: React.CSSProperties = {
  padding: '14px 24px 22px',
  display: 'flex',
  flexDirection: 'column',
  gap: 14,
};
const cardTitleStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 17,
  fontWeight: 600,
  letterSpacing: '-0.01em',
};
const mutedStyle: React.CSSProperties = {
  margin: 0,
  color: 'var(--fg-muted)',
  fontSize: 13,
};
const sharePillStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 12,
  padding: '8px 12px',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--r-sm)',
};
const formRowStyle: React.CSSProperties = {
  display: 'flex',
  gap: 8,
  alignItems: 'center',
  flexWrap: 'wrap',
};

const STATUS_MESSAGES: Record<string, { text: string; tone: 'ok' | 'danger' }> = {
  visibility_public: { text: 'Profile is now public.', tone: 'ok' },
  visibility_private: { text: 'Profile is now private.', tone: 'ok' },
  share_added: { text: 'Share granted.', tone: 'ok' },
  share_revoked: { text: 'Share revoked.', tone: 'ok' },
  org_share_added: { text: 'Org share granted.', tone: 'ok' },
  org_share_revoked: { text: 'Org share revoked.', tone: 'ok' },
};

const ERROR_MESSAGES: Record<string, string> = {
  rsi_handle_not_verified:
    'Verify your RSI handle in Settings before granting shares.',
  recipient_not_found: 'No StarStats account exists for that handle.',
  org_not_found: 'No org exists with that slug.',
  invalid_recipient_handle: 'Handle looks invalid — letters, digits, dashes only.',
  invalid_org_slug: 'Org slug looks invalid.',
  cannot_share_with_self: "You can't share your stats with yourself.",
  spicedb_unavailable:
    'The authorisation service is offline. Try again shortly.',
  unexpected: 'Something went wrong. Try again.',
};

export default async function SharingPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/sharing');

  const params = await props.searchParams;
  const status = params.status;
  const errorCode = params.error;
  const prefilledHandle = (params.handle ?? '').trim();

  // Load the four parallel data sources. SpiceDB outages map to
  // `degraded` so the page still renders with a clear banner instead
  // of crashing.
  let visibility: VisibilityResponse | null = null;
  let shares: ListSharesResponse | null = null;
  let inbound: ListSharedWithMeResponse | null = null;
  let myOrgs: ListOrgsResponse | null = null;
  let degraded: 'spicedb_unavailable' | 'unknown' | null = null;
  try {
    [visibility, shares, inbound, myOrgs] = await Promise.all([
      getVisibility(session.token),
      listShares(session.token),
      listSharedWithMe(session.token),
      listOrgs(session.token),
    ]);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/sharing');
    }
    if (e instanceof ApiCallError && e.status === 503) {
      degraded = 'spicedb_unavailable';
    } else {
      logger.error({ err: e }, 'load sharing state failed');
      degraded = 'unknown';
    }
  }

  // -- Server actions --------------------------------------------------

  async function visibilityAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/sharing');
    const wantPublic = String(formData.get('public') ?? 'false') === 'true';
    try {
      await setVisibility(s.token, wantPublic);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401)
        redirect('/auth/login?next=/sharing');
      if (e instanceof ApiCallError && e.status === 403)
        redirect('/sharing?error=rsi_handle_not_verified');
      if (e instanceof ApiCallError && e.status === 503)
        redirect('/sharing?error=spicedb_unavailable');
      logger.error({ err: e }, 'set visibility failed');
      redirect('/sharing?error=unexpected');
    }
    redirect(
      `/sharing?status=visibility_${wantPublic ? 'public' : 'private'}`,
    );
  }

  async function addShareAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/sharing');
    const recipient = String(formData.get('recipient_handle') ?? '').trim();
    if (recipient === '') redirect('/sharing?error=invalid_recipient_handle');
    try {
      await addShare(s.token, recipient);
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/sharing');
        if (e.status === 403) redirect('/sharing?error=rsi_handle_not_verified');
        if (e.status === 404) redirect('/sharing?error=recipient_not_found');
        if (e.status === 400)
          redirect(`/sharing?error=${encodeURIComponent(e.body.error)}`);
        if (e.status === 503) redirect('/sharing?error=spicedb_unavailable');
      }
      logger.error({ err: e }, 'add share failed');
      redirect('/sharing?error=unexpected');
    }
    redirect('/sharing?status=share_added');
  }

  async function revokeShareAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/sharing');
    const recipient = String(formData.get('recipient_handle') ?? '').trim();
    if (recipient === '') redirect('/sharing?error=invalid_recipient_handle');
    try {
      await removeShare(s.token, recipient);
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/sharing');
        if (e.status === 503) redirect('/sharing?error=spicedb_unavailable');
      }
      logger.error({ err: e }, 'remove share failed');
      redirect('/sharing?error=unexpected');
    }
    redirect('/sharing?status=share_revoked');
  }

  async function shareOrgAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/sharing');
    const slug = String(formData.get('org_slug') ?? '').trim();
    if (slug === '') redirect('/sharing?error=invalid_org_slug');
    try {
      await shareWithOrg(s.token, slug);
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/sharing');
        if (e.status === 403) redirect('/sharing?error=rsi_handle_not_verified');
        if (e.status === 404) redirect('/sharing?error=org_not_found');
        if (e.status === 400)
          redirect(`/sharing?error=${encodeURIComponent(e.body.error)}`);
        if (e.status === 503) redirect('/sharing?error=spicedb_unavailable');
      }
      logger.error({ err: e }, 'share with org failed');
      redirect('/sharing?error=unexpected');
    }
    redirect('/sharing?status=org_share_added');
  }

  async function revokeOrgShareAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/sharing');
    const slug = String(formData.get('org_slug') ?? '').trim();
    if (slug === '') redirect('/sharing?error=invalid_org_slug');
    try {
      await unshareWithOrg(s.token, slug);
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/sharing');
        if (e.status === 503) redirect('/sharing?error=spicedb_unavailable');
      }
      logger.error({ err: e }, 'remove org share failed');
      redirect('/sharing?error=unexpected');
    }
    redirect('/sharing?status=org_share_revoked');
  }

  // -- Render ----------------------------------------------------------

  return (
    <div className="ss-screen-enter" style={pageStyle}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Community · who can see your manifest
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Sharing
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
          }}
        >
          Make your profile public, grant view-access to specific handles
          and orgs, and see who has shared their manifest with you.
        </p>
      </header>

      {status && STATUS_MESSAGES[status] && (
        <div
          role="status"
          className={`ss-badge ${
            STATUS_MESSAGES[status].tone === 'ok' ? 'ss-badge--ok' : ''
          }`}
          style={{ alignSelf: 'flex-start' }}
        >
          {STATUS_MESSAGES[status].text}
        </div>
      )}
      {errorCode && (
        <div
          role="alert"
          className="ss-badge"
          style={{
            alignSelf: 'flex-start',
            borderColor: 'var(--danger)',
            color: 'var(--danger)',
          }}
        >
          {ERROR_MESSAGES[errorCode] ?? errorCode}
        </div>
      )}

      {degraded === 'spicedb_unavailable' ? (
        <section className="ss-card">
          <div style={cardBodyStyle}>
            <p style={mutedStyle}>
              Sharing is temporarily unavailable — the authorisation
              service is offline. Try again shortly.
            </p>
          </div>
        </section>
      ) : degraded === 'unknown' ? (
        <section className="ss-card">
          <div style={cardBodyStyle}>
            <p style={mutedStyle}>
              Couldn&apos;t load your sharing state. Refresh to retry — if it
              keeps failing, please report it.
            </p>
          </div>
        </section>
      ) : (
        <>
          {/* Visibility toggle ------------------------------------------ */}
          <section className="ss-card">
            <header style={cardHeaderStyle}>
              <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
                Visibility
              </div>
              <h2 style={cardTitleStyle}>Public profile</h2>
            </header>
            <div style={cardBodyStyle}>
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'flex-start',
                  gap: 16,
                  flexWrap: 'wrap',
                }}
              >
                <p style={{ ...mutedStyle, flex: 1, minWidth: 240 }}>
                  When public, anyone can view your summary and timeline
                  at{' '}
                  <Link
                    href={
                      (`/u/${encodeURIComponent(session.claimedHandle)}`) as Route
                    }
                    className="mono"
                    style={{ color: 'var(--fg)' }}
                  >
                    /u/{session.claimedHandle}
                  </Link>
                  .
                </p>
                <div
                  style={{ display: 'flex', alignItems: 'center', gap: 10 }}
                >
                  <span
                    className={`ss-badge ${visibility?.public ? 'ss-badge--ok' : ''}`}
                  >
                    {visibility?.public ? (
                      <>
                        <span className="ss-badge-dot" />
                        Public
                      </>
                    ) : (
                      'Private'
                    )}
                  </span>
                  <form action={visibilityAction} style={{ margin: 0 }}>
                    <input
                      type="hidden"
                      name="public"
                      value={visibility?.public ? 'false' : 'true'}
                    />
                    <button type="submit" className="ss-btn ss-btn--ghost">
                      {visibility?.public ? 'Make private' : 'Make public'}
                    </button>
                  </form>
                </div>
              </div>
            </div>
          </section>

          {/* Outbound — handles ----------------------------------------- */}
          <section className="ss-card">
            <header style={cardHeaderStyle}>
              <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
                Outbound · handles
              </div>
              <h2 style={cardTitleStyle}>Shared with specific handles</h2>
            </header>
            <div style={cardBodyStyle}>
              {shares && shares.shares.length > 0 ? (
                <div
                  style={{ display: 'flex', flexDirection: 'column', gap: 8 }}
                >
                  {shares.shares.map((entry) => (
                    <div key={entry.recipient_handle} style={sharePillStyle}>
                      <Link
                        href={
                          (`/u/${encodeURIComponent(entry.recipient_handle)}`) as Route
                        }
                        className="mono"
                        style={{ color: 'var(--fg)' }}
                      >
                        {entry.recipient_handle}
                      </Link>
                      <form action={revokeShareAction} style={{ margin: 0 }}>
                        <input
                          type="hidden"
                          name="recipient_handle"
                          value={entry.recipient_handle}
                        />
                        <button
                          type="submit"
                          className="ss-btn ss-btn--link"
                          style={{ color: 'var(--danger)' }}
                        >
                          Revoke
                        </button>
                      </form>
                    </div>
                  ))}
                </div>
              ) : (
                <p style={mutedStyle}>
                  You haven&apos;t shared with any specific handles yet.
                </p>
              )}
              <form action={addShareAction} style={formRowStyle}>
                <input
                  type="text"
                  name="recipient_handle"
                  placeholder="RSI handle"
                  defaultValue={prefilledHandle}
                  autoComplete="off"
                  spellCheck={false}
                  className="mono"
                  required
                  maxLength={64}
                  style={{
                    flex: '1 1 220px',
                    padding: '8px 12px',
                    background: 'var(--bg-elev)',
                    border: '1px solid var(--border)',
                    borderRadius: 'var(--r-sm)',
                    color: 'var(--fg)',
                  }}
                />
                <button type="submit" className="ss-btn ss-btn--primary">
                  Grant access
                </button>
              </form>
            </div>
          </section>

          {/* Outbound — orgs -------------------------------------------- */}
          <section className="ss-card">
            <header style={cardHeaderStyle}>
              <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
                Outbound · orgs
              </div>
              <h2 style={cardTitleStyle}>Shared with orgs</h2>
            </header>
            <div style={cardBodyStyle}>
              {shares && (shares.org_shares?.length ?? 0) > 0 ? (
                <div
                  style={{ display: 'flex', flexDirection: 'column', gap: 8 }}
                >
                  {(shares.org_shares ?? []).map((entry) => (
                    <div key={entry.org_slug} style={sharePillStyle}>
                      <Link
                        href={
                          (`/orgs/${encodeURIComponent(entry.org_slug)}`) as Route
                        }
                        className="mono"
                        style={{ color: 'var(--fg)' }}
                      >
                        {entry.org_slug}
                      </Link>
                      <form action={revokeOrgShareAction} style={{ margin: 0 }}>
                        <input
                          type="hidden"
                          name="org_slug"
                          value={entry.org_slug}
                        />
                        <button
                          type="submit"
                          className="ss-btn ss-btn--link"
                          style={{ color: 'var(--danger)' }}
                        >
                          Revoke
                        </button>
                      </form>
                    </div>
                  ))}
                </div>
              ) : (
                <p style={mutedStyle}>
                  You haven&apos;t shared with any orgs yet.
                </p>
              )}
              {myOrgs && myOrgs.orgs.length > 0 ? (
                <form action={shareOrgAction} style={formRowStyle}>
                  <select
                    name="org_slug"
                    required
                    className="mono"
                    defaultValue=""
                    style={{
                      flex: '1 1 220px',
                      padding: '8px 12px',
                      background: 'var(--bg-elev)',
                      border: '1px solid var(--border)',
                      borderRadius: 'var(--r-sm)',
                      color: 'var(--fg)',
                    }}
                  >
                    <option value="" disabled>
                      Pick one of your orgs…
                    </option>
                    {myOrgs.orgs.map((o) => (
                      <option key={o.slug} value={o.slug}>
                        {o.name} ({o.slug})
                      </option>
                    ))}
                  </select>
                  <button type="submit" className="ss-btn ss-btn--primary">
                    Grant access
                  </button>
                </form>
              ) : (
                <p style={mutedStyle}>
                  You&apos;re not in any orgs yet —{' '}
                  <Link href="/orgs/new">create one</Link> to share by org.
                </p>
              )}
            </div>
          </section>

          {/* Inbound — what's been shared with me ------------------------ */}
          <section className="ss-card">
            <header style={cardHeaderStyle}>
              <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
                Inbound · shared with you
              </div>
              <h2 style={cardTitleStyle}>People sharing with you</h2>
            </header>
            <div style={cardBodyStyle}>
              <p style={mutedStyle}>
                These owners have granted you view-access to their manifest.
                Org-mediated shares (via shared orgs) aren&apos;t listed here —
                check the org&apos;s detail page for those.
              </p>
              {inbound && inbound.shared_with_me.length > 0 ? (
                <div
                  style={{ display: 'flex', flexDirection: 'column', gap: 8 }}
                >
                  {inbound.shared_with_me.map((entry) => (
                    <div key={entry.owner_handle} style={sharePillStyle}>
                      <Link
                        href={
                          (`/u/${encodeURIComponent(entry.owner_handle)}`) as Route
                        }
                        className="mono"
                        style={{ color: 'var(--fg)' }}
                      >
                        @{entry.owner_handle}
                      </Link>
                      <Link
                        href={
                          (`/u/${encodeURIComponent(entry.owner_handle)}`) as Route
                        }
                        className="ss-btn ss-btn--link"
                      >
                        View profile
                      </Link>
                    </div>
                  ))}
                </div>
              ) : (
                <p style={mutedStyle}>
                  Nobody has shared their manifest with you yet.
                </p>
              )}
            </div>
          </section>
        </>
      )}
    </div>
  );
}
