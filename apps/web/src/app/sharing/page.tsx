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
  type ShareScope,
  type SharedWithMeEntry,
  type VisibilityResponse,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';
import { reportShareAction } from './actions';

interface SearchParams {
  status?: string;
  error?: string;
  /** Pre-populate the add-handle field — set by per-profile "Share back" CTA. */
  handle?: string;
  /**
   * Pre-populate the optional expiry field for in-place edit. Format
   * is the `<input type="datetime-local">` shape (`YYYY-MM-DDTHH:MM`
   * in local time) — what the browser already submits, and what the
   * edit-Link writes back into the URL.
   */
  expires?: string;
  /** Pre-populate the note field for in-place edit. */
  note?: string;
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
  expires_at_in_past: 'Expiry must be in the future.',
  note_too_long: 'Note is too long (max 280 characters).',
  invalid_scope_kind: 'Pick a valid scope kind.',
  invalid_scope_window: 'Scope window must be between 1 and 90 days.',
  invalid_scope_tabs: 'One of the selected tabs is unknown.',
  invalid_scope_types: 'Event-type filter contains invalid entries.',
  spicedb_unavailable:
    'The authorisation service is offline. Try again shortly.',
  unexpected: 'Something went wrong. Try again.',
};

/** Closed vocabulary mirroring `ALLOWED_SCOPE_TABS` in the Rust
 *  validator. Centralising both lists in the page makes it cheap to
 *  add a new tab — bump both sides and the picker just works. */
const SCOPE_TAB_OPTIONS: ReadonlyArray<{ value: string; label: string }> = [
  { value: 'location', label: 'Location' },
  { value: 'travel', label: 'Travel' },
  { value: 'combat', label: 'Combat' },
  { value: 'loadout', label: 'Loadout' },
  { value: 'stability', label: 'Stability' },
  { value: 'commerce', label: 'Commerce' },
];

/** Format a timestamp as a short relative string like "3d ago" or
 *  "just now". Returns null for missing input so the caller can
 *  conditionally render. */
function formatRelativePast(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const ts = new Date(iso);
  if (Number.isNaN(ts.getTime())) return null;
  const diffMs = Date.now() - ts.getTime();
  if (diffMs < 0) return 'just now';
  const min = Math.round(diffMs / 60_000);
  if (min < 1) return 'just now';
  if (min < 60) return `${min}m ago`;
  const hr = Math.round(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.round(hr / 24);
  return `${day}d ago`;
}

/**
 * Build the per-pill "Edit" URL. Round-trips the share's current
 * expiry + note through the URL so the existing add-share form can
 * pre-fill them; submitting that form re-POSTs to /v1/me/share which
 * upserts the metadata (set + clear are both supported now). The
 * expiry is serialised as the `<input type="datetime-local">` shape
 * (`YYYY-MM-DDTHH:MM`, no timezone) — that's what the input expects
 * and what the server-action already converts back via `new Date()`.
 */
function buildEditHref(
  recipientHandle: string,
  expiresAt: string | null | undefined,
  note: string | null | undefined,
): string {
  const qs = new URLSearchParams();
  qs.set('handle', recipientHandle);
  if (expiresAt) {
    const dt = new Date(expiresAt);
    if (!Number.isNaN(dt.getTime())) {
      // toISOString → UTC `YYYY-MM-DDTHH:MM:SS.sssZ`; slice down to
      // the datetime-local shape. Matches what the form already
      // submits, so the round-trip is symmetrical even if the user
      // doesn't touch the field.
      qs.set('expires', dt.toISOString().slice(0, 16));
    }
  }
  if (note) qs.set('note', note);
  return `/sharing?${qs.toString()}#share-editor`;
}

/** Format an ISO timestamp as "in 3d" / "expired" / "in 2h" for the
 *  share pills. Returns null when no expiry was set. */
function formatExpiry(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const ts = new Date(iso);
  if (Number.isNaN(ts.getTime())) return null;
  const now = Date.now();
  const diffMs = ts.getTime() - now;
  if (diffMs <= 0) return 'expired';
  const diffMin = Math.round(diffMs / 60_000);
  if (diffMin < 60) return `in ${diffMin}m`;
  const diffHr = Math.round(diffMin / 60);
  if (diffHr < 24) return `in ${diffHr}h`;
  const diffDay = Math.round(diffHr / 24);
  return `in ${diffDay}d`;
}

export default async function SharingPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/sharing');

  const params = await props.searchParams;
  const status = params.status;
  const errorCode = params.error;
  const prefilledHandle = (params.handle ?? '').trim();
  const prefilledExpires = (params.expires ?? '').trim();
  const prefilledNote = (params.note ?? '').trim();
  // "Edit mode" = any of the prefill fields are set. Switches the
  // form's title/button copy from "grant" to "save changes" so the
  // user understands they're updating an existing row.
  const isEditing = prefilledHandle !== '';

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
    // Optional expiry comes from an <input type="datetime-local">,
    // which returns a naive local string like "2026-06-01T10:00".
    // Convert to ISO with the browser's timezone offset baked in
    // so the server can compare against UTC. Empty = no expiry.
    const expiresLocal = String(formData.get('expires_at_local') ?? '').trim();
    let expiresAt: string | null = null;
    if (expiresLocal !== '') {
      const dt = new Date(expiresLocal);
      if (!Number.isNaN(dt.getTime())) {
        expiresAt = dt.toISOString();
      }
    }
    const noteRaw = String(formData.get('note') ?? '').trim();
    const note = noteRaw === '' ? null : noteRaw;
    // Build the scope payload from the picker. `kind="full"` (or
    // absent) is the legacy default and skips the wire field — the
    // server normalises kind=full back to NULL anyway, but keeping
    // the body minimal makes the audit-log payload easier to read.
    const scopeKind = String(formData.get('scope_kind') ?? 'full').trim();
    let scope: ShareScope | null = null;
    if (scopeKind && scopeKind !== 'full') {
      const tabs = formData.getAll('scope_tabs').map(String).filter(Boolean);
      const windowDaysRaw = String(formData.get('scope_window_days') ?? '').trim();
      const windowDays = windowDaysRaw === '' ? null : Number(windowDaysRaw);
      const denyRaw = String(formData.get('scope_deny_event_types') ?? '').trim();
      const allowRaw = String(formData.get('scope_allow_event_types') ?? '').trim();
      // Comma-separated, lowercased, deduped. Empty list -> null so
      // we don't ship `[]` and trigger a "list too long" code-path
      // false positive on the server.
      const parseTypeList = (raw: string): string[] | null => {
        const parts = raw
          .split(',')
          .map((s) => s.trim().toLowerCase())
          .filter((s) => s.length > 0);
        return parts.length === 0 ? null : Array.from(new Set(parts));
      };
      scope = {
        kind: scopeKind,
        tabs: scopeKind === 'tabs' && tabs.length > 0 ? tabs : null,
        window_days:
          windowDays !== null && Number.isFinite(windowDays) ? windowDays : null,
        allow_event_types: parseTypeList(allowRaw),
        deny_event_types: parseTypeList(denyRaw),
      };
    }
    try {
      await addShare(s.token, recipient, { expiresAt, note, scope });
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
          {/* Visibility — audit §05.7 calls for this to read as a top-
              level section header with the toggle inline, not as a buried
              card body row. Public state additionally surfaces the
              shareable URL inline so the user can copy it without
              hopping to the profile page. */}
          <section
            className="ss-card"
            aria-labelledby="ss-visibility-heading"
          >
            <header
              style={{
                ...cardHeaderStyle,
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: 16,
                flexWrap: 'wrap',
                paddingBottom: 14,
              }}
            >
              <div style={{ minWidth: 240, flex: 1 }}>
                <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
                  Visibility
                </div>
                <h2 id="ss-visibility-heading" style={cardTitleStyle}>
                  {visibility?.public
                    ? 'Profile is public'
                    : 'Profile is private'}
                </h2>
              </div>
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
                  <button
                    type="submit"
                    className={`ss-btn ${visibility?.public ? 'ss-btn--ghost' : 'ss-btn--primary'}`}
                  >
                    {visibility?.public ? 'Make private' : 'Make public'}
                  </button>
                </form>
              </div>
            </header>
            <div style={cardBodyStyle}>
              <p style={mutedStyle}>
                When public, anyone can view your summary and timeline at
                the URL below.
              </p>
              {visibility?.public ? (
                <div
                  style={{
                    display: 'flex',
                    gap: 8,
                    alignItems: 'center',
                    flexWrap: 'wrap',
                  }}
                >
                  <input
                    readOnly
                    value={`/u/${session.claimedHandle}`}
                    aria-label="Shareable public URL"
                    className="mono"
                    style={{
                      flex: '1 1 280px',
                      padding: '8px 12px',
                      background: 'var(--bg-elev)',
                      border: '1px solid var(--border)',
                      borderRadius: 'var(--r-sm)',
                      color: 'var(--fg)',
                      fontSize: 13,
                    }}
                  />
                  <Link
                    href={
                      (`/u/${encodeURIComponent(session.claimedHandle)}`) as Route
                    }
                    className="ss-btn ss-btn--ghost"
                    style={{ textDecoration: 'none' }}
                  >
                    Open
                  </Link>
                </div>
              ) : null}
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
                  {shares.shares.map((entry) => {
                    const expiryLabel = formatExpiry(entry.expires_at);
                    // Audit v2 §05.2 — owner-visible activity hint
                    // beneath each pill. View_count + last_viewed_at
                    // come from the audit-log GROUP BY done server-
                    // side; we just render them.
                    const lastViewed = formatRelativePast(entry.last_viewed_at);
                    const viewCount = entry.view_count ?? 0;
                    const activityBits: string[] = [];
                    if (viewCount === 0) {
                      activityBits.push('not yet viewed');
                    } else {
                      activityBits.push(
                        `viewed ${viewCount} ${viewCount === 1 ? 'time' : 'times'}`,
                      );
                      if (lastViewed) activityBits.push(`last ${lastViewed}`);
                    }
                    if (entry.scope?.kind && entry.scope.kind !== 'full') {
                      activityBits.push(`scope: ${entry.scope.kind}`);
                    }
                    return (
                      <div
                        key={entry.recipient_handle}
                        style={{ ...sharePillStyle, flexWrap: 'wrap' }}
                      >
                        <div
                          style={{
                            display: 'flex',
                            flexDirection: 'column',
                            gap: 2,
                            flex: 1,
                            minWidth: 0,
                          }}
                        >
                          <Link
                            href={
                              (`/u/${encodeURIComponent(entry.recipient_handle)}`) as Route
                            }
                            className="mono"
                            style={{ color: 'var(--fg)' }}
                          >
                            {entry.recipient_handle}
                          </Link>
                          {entry.note && (
                            <span
                              style={{
                                fontSize: 12,
                                color: 'var(--fg-muted)',
                              }}
                            >
                              {entry.note}
                            </span>
                          )}
                          <span
                            style={{
                              fontSize: 11,
                              color: 'var(--fg-muted)',
                              letterSpacing: '0.01em',
                            }}
                            title={entry.last_viewed_at ?? undefined}
                          >
                            {activityBits.join(' · ')}
                          </span>
                        </div>
                        {expiryLabel && (
                          <span
                            className="ss-badge"
                            title={entry.expires_at ?? undefined}
                            style={
                              expiryLabel === 'expired'
                                ? { borderColor: 'var(--danger)', color: 'var(--danger)' }
                                : { color: 'var(--fg-muted)' }
                            }
                          >
                            {expiryLabel === 'expired' ? 'expired' : `expires ${expiryLabel}`}
                          </span>
                        )}
                        <Link
                          href={
                            buildEditHref(
                              entry.recipient_handle,
                              entry.expires_at,
                              entry.note,
                            ) as Route
                          }
                          className="ss-btn ss-btn--link"
                        >
                          Edit
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
                    );
                  })}
                </div>
              ) : (
                <p style={mutedStyle}>
                  You haven&apos;t shared with any specific handles yet.
                </p>
              )}
              <form
                id="share-editor"
                action={addShareAction}
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 8,
                }}
              >
                {isEditing && (
                  <p
                    style={{
                      ...mutedStyle,
                      fontSize: 12,
                      color: 'var(--accent)',
                    }}
                  >
                    Editing share with <span className="mono">{prefilledHandle}</span>
                    {' '}— blank out a field and save to clear it.
                  </p>
                )}
                <div style={formRowStyle}>
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
                    readOnly={isEditing}
                    style={{
                      flex: '1 1 220px',
                      padding: '8px 12px',
                      background: 'var(--bg-elev)',
                      border: '1px solid var(--border)',
                      borderRadius: 'var(--r-sm)',
                      color: 'var(--fg)',
                      opacity: isEditing ? 0.7 : 1,
                    }}
                  />
                  <input
                    type="datetime-local"
                    name="expires_at_local"
                    defaultValue={prefilledExpires}
                    aria-label="Auto-expiry (optional)"
                    title="Leave blank for no expiry"
                    style={{
                      flex: '0 1 220px',
                      padding: '8px 12px',
                      background: 'var(--bg-elev)',
                      border: '1px solid var(--border)',
                      borderRadius: 'var(--r-sm)',
                      color: 'var(--fg)',
                    }}
                  />
                  <button type="submit" className="ss-btn ss-btn--primary">
                    {isEditing ? 'Save changes' : 'Grant access'}
                  </button>
                  {isEditing && (
                    <Link
                      href={'/sharing' as Route}
                      className="ss-btn ss-btn--ghost"
                      style={{ textDecoration: 'none' }}
                    >
                      Cancel
                    </Link>
                  )}
                </div>
                <input
                  type="text"
                  name="note"
                  placeholder="Note (optional, max 280 chars)"
                  defaultValue={prefilledNote}
                  maxLength={280}
                  aria-label="Note (optional)"
                  style={{
                    padding: '8px 12px',
                    background: 'var(--bg-elev)',
                    border: '1px solid var(--border)',
                    borderRadius: 'var(--r-sm)',
                    color: 'var(--fg)',
                  }}
                />
                {/* Scope picker (audit §05.1) — hidden behind a
                    `<details>` so the existing two-line grant form
                    stays the default. Pure native HTML so the page
                    can remain a server component; the server action
                    above reads the values straight off FormData. */}
                <details style={{ marginTop: 4 }}>
                  <summary
                    style={{
                      cursor: 'pointer',
                      fontSize: 12,
                      color: 'var(--fg-muted)',
                      padding: '4px 0',
                    }}
                  >
                    Customise scope — default is full manifest
                  </summary>
                  <div
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 10,
                      padding: '10px 0 4px',
                    }}
                  >
                    <label
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 4,
                        fontSize: 12,
                        color: 'var(--fg-muted)',
                      }}
                    >
                      Scope kind
                      <select
                        name="scope_kind"
                        defaultValue="full"
                        style={{
                          padding: '8px 12px',
                          background: 'var(--bg-elev)',
                          border: '1px solid var(--border)',
                          borderRadius: 'var(--r-sm)',
                          color: 'var(--fg)',
                        }}
                      >
                        <option value="full">Full manifest (default)</option>
                        <option value="timeline">Timeline only</option>
                        <option value="aggregates">Aggregates only</option>
                        <option value="tabs">Specific tabs…</option>
                      </select>
                    </label>
                    <fieldset
                      style={{
                        border: '1px solid var(--border)',
                        borderRadius: 'var(--r-sm)',
                        padding: '8px 12px',
                        margin: 0,
                      }}
                    >
                      <legend
                        style={{
                          fontSize: 12,
                          color: 'var(--fg-muted)',
                          padding: '0 4px',
                        }}
                      >
                        Tabs (used when scope kind = tabs)
                      </legend>
                      <div
                        style={{
                          display: 'flex',
                          gap: 12,
                          flexWrap: 'wrap',
                        }}
                      >
                        {SCOPE_TAB_OPTIONS.map((t) => (
                          <label
                            key={t.value}
                            style={{
                              display: 'inline-flex',
                              alignItems: 'center',
                              gap: 6,
                              fontSize: 13,
                            }}
                          >
                            <input
                              type="checkbox"
                              name="scope_tabs"
                              value={t.value}
                            />
                            {t.label}
                          </label>
                        ))}
                      </div>
                    </fieldset>
                    <label
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 4,
                        fontSize: 12,
                        color: 'var(--fg-muted)',
                      }}
                    >
                      Window (days, 1–90 — blank = no clamp)
                      <input
                        type="number"
                        name="scope_window_days"
                        min={1}
                        max={90}
                        placeholder="e.g. 7"
                        style={{
                          padding: '8px 12px',
                          background: 'var(--bg-elev)',
                          border: '1px solid var(--border)',
                          borderRadius: 'var(--r-sm)',
                          color: 'var(--fg)',
                        }}
                      />
                    </label>
                    <label
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 4,
                        fontSize: 12,
                        color: 'var(--fg-muted)',
                      }}
                    >
                      Allow event types (comma-separated, blank = all)
                      <input
                        type="text"
                        name="scope_allow_event_types"
                        placeholder="quantum_target_selected, jump_completed"
                        className="mono"
                        style={{
                          padding: '8px 12px',
                          background: 'var(--bg-elev)',
                          border: '1px solid var(--border)',
                          borderRadius: 'var(--r-sm)',
                          color: 'var(--fg)',
                        }}
                      />
                    </label>
                    <label
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 4,
                        fontSize: 12,
                        color: 'var(--fg-muted)',
                      }}
                    >
                      Deny event types (comma-separated)
                      <input
                        type="text"
                        name="scope_deny_event_types"
                        placeholder="actor_death"
                        className="mono"
                        style={{
                          padding: '8px 12px',
                          background: 'var(--bg-elev)',
                          border: '1px solid var(--border)',
                          borderRadius: 'var(--r-sm)',
                          color: 'var(--fg)',
                        }}
                      />
                    </label>
                  </div>
                </details>
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
                  {/* Resolve org name from the user's own org list so the
                      pill is readable ("Aurora Wing", not just
                      "aurora-wing"). When the user has shared with an
                      org they're no longer a member of, the lookup
                      misses and the slug stands in — clear enough that
                      the row is still actionable. */}
                  {(() => {
                    const orgNameBySlug = new Map(
                      (myOrgs?.orgs ?? []).map((o) => [o.slug, o.name]),
                    );
                    return (shares.org_shares ?? []).map((entry) => {
                      const name = orgNameBySlug.get(entry.org_slug);
                      return (
                        <div key={entry.org_slug} style={sharePillStyle}>
                          <Link
                            href={
                              (`/orgs/${encodeURIComponent(entry.org_slug)}`) as Route
                            }
                            style={{
                              display: 'flex',
                              flexDirection: 'column',
                              gap: 2,
                              color: 'var(--fg)',
                              textDecoration: 'none',
                              minWidth: 0,
                            }}
                          >
                            <span style={{ fontWeight: 500 }}>
                              {name ?? entry.org_slug}
                            </span>
                            {name && (
                              <span
                                className="mono"
                                style={{
                                  fontSize: 11,
                                  color: 'var(--fg-muted)',
                                }}
                              >
                                {entry.org_slug}
                              </span>
                            )}
                          </Link>
                          <form
                            action={revokeOrgShareAction}
                            style={{ margin: 0 }}
                          >
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
                      );
                    });
                  })()}
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
              {(() => {
                const entries = inbound?.shared_with_me ?? [];
                const now = Date.now();
                const isExpired = (e: typeof entries[number]) =>
                  e.expires_at !== null &&
                  e.expires_at !== undefined &&
                  new Date(e.expires_at).getTime() <= now;
                const active = entries.filter((e) => !isExpired(e));
                const expired = entries.filter(isExpired);

                if (entries.length === 0) {
                  return (
                    <p style={mutedStyle}>
                      Nobody has shared their manifest with you yet.
                    </p>
                  );
                }

                return (
                  <>
                    {active.length > 0 && (
                      <div
                        style={{
                          display: 'flex',
                          flexDirection: 'column',
                          gap: 8,
                        }}
                      >
                        {active.map((entry) => (
                          <InboundPill
                            key={entry.owner_handle}
                            entry={entry}
                            recipientHandle={session.claimedHandle}
                          />
                        ))}
                      </div>
                    )}
                    {expired.length > 0 && (
                      <details
                        style={{ marginTop: active.length > 0 ? 10 : 0 }}
                      >
                        <summary
                          style={{
                            cursor: 'pointer',
                            fontSize: 12,
                            color: 'var(--fg-muted)',
                            padding: '4px 0',
                          }}
                        >
                          {expired.length} expired{' '}
                          {expired.length === 1 ? 'share' : 'shares'} — owner
                          set an expiry that has now passed
                        </summary>
                        <div
                          style={{
                            display: 'flex',
                            flexDirection: 'column',
                            gap: 8,
                            marginTop: 8,
                            opacity: 0.55,
                          }}
                        >
                          {expired.map((entry) => (
                            <InboundPill
                              key={entry.owner_handle}
                              entry={entry}
                              recipientHandle={session.claimedHandle}
                            />
                          ))}
                        </div>
                      </details>
                    )}
                    {active.length === 0 && expired.length > 0 && (
                      <p style={mutedStyle}>
                        Every share you have has expired. Nothing new right
                        now.
                      </p>
                    )}
                  </>
                );
              })()}
            </div>
          </section>
        </>
      )}
    </div>
  );
}

/**
 * Inbound share row. Extracted because both the active list and the
 * collapsed expired-shares group render the same pill — keeping the
 * markup in one place is the only way the two sub-lists stay visually
 * aligned as styling evolves.
 */
function InboundPill({
  entry,
  recipientHandle,
}: {
  entry: SharedWithMeEntry;
  recipientHandle: string;
}) {
  const expiryLabel = formatExpiry(entry.expires_at);
  return (
    <div
      style={{
        ...sharePillStyle,
        flexWrap: 'wrap',
        flexDirection: 'column',
        alignItems: 'stretch',
        gap: 8,
      }}
    >
      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 2,
            flex: 1,
            minWidth: 0,
          }}
        >
          <Link
            href={(`/u/${encodeURIComponent(entry.owner_handle)}`) as Route}
            className="mono"
            style={{ color: 'var(--fg)' }}
          >
            @{entry.owner_handle}
          </Link>
          {entry.note && (
            <span style={{ fontSize: 12, color: 'var(--fg-muted)' }}>
              {entry.note}
            </span>
          )}
        </div>
        {expiryLabel && (
          <span
            className="ss-badge"
            title={entry.expires_at ?? undefined}
            style={
              expiryLabel === 'expired'
                ? { borderColor: 'var(--danger)', color: 'var(--danger)' }
                : { color: 'var(--fg-muted)' }
            }
          >
            {expiryLabel === 'expired' ? 'expired' : `expires ${expiryLabel}`}
          </span>
        )}
        <Link
          href={(`/u/${encodeURIComponent(entry.owner_handle)}`) as Route}
          className="ss-btn ss-btn--link"
        >
          View profile
        </Link>
      </div>

      {/* Audit v2 §05 — recipient-facing report affordance. Collapsed
          by default so the row stays compact; opens an inline form
          that posts to /v1/share/report via the server action. */}
      <details>
        <summary
          style={{
            cursor: 'pointer',
            fontSize: 12,
            color: 'var(--fg-muted)',
          }}
        >
          Report this share
        </summary>
        <form
          action={reportShareAction}
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 6,
            marginTop: 8,
            padding: 10,
            border: '1px solid var(--border)',
            borderRadius: 'var(--r-card)',
            background: 'var(--bg-sunken)',
          }}
        >
          <input
            type="hidden"
            name="owner_handle"
            value={entry.owner_handle}
          />
          <input
            type="hidden"
            name="recipient_handle"
            value={recipientHandle}
          />
          <label
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
              fontSize: 12,
            }}
          >
            <span style={{ color: 'var(--fg-muted)' }}>Reason</span>
            <select
              name="reason"
              required
              defaultValue="abuse"
              style={{
                padding: 6,
                background: 'var(--bg-elev)',
                color: 'var(--fg)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--r-input)',
                fontSize: 13,
              }}
            >
              <option value="abuse">Abuse</option>
              <option value="spam">Spam</option>
              <option value="data_misuse">Data misuse</option>
              <option value="other">Other</option>
            </select>
          </label>
          <label
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
              fontSize: 12,
            }}
          >
            <span style={{ color: 'var(--fg-muted)' }}>
              Details (optional, ≤ 500 chars)
            </span>
            <textarea
              name="details"
              rows={2}
              maxLength={500}
              style={{
                resize: 'vertical',
                fontFamily: 'inherit',
                fontSize: 13,
                padding: 6,
                background: 'var(--bg-elev)',
                color: 'var(--fg)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--r-input)',
              }}
            />
          </label>
          <button type="submit" className="ss-btn">
            Submit report
          </button>
        </form>
      </details>
    </div>
  );
}
