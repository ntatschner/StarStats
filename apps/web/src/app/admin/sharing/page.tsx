/**
 * Admin · Sharing overview.
 *
 * First of the five views called for in audit §05 ("Admin side · what
 * to build"). Wave W3 ships two:
 *   1. /admin/sharing            — this page (overview)
 *   2. /admin/sharing/audit      — sharing-scoped audit log
 *
 * Deferred to a later wave (TODO: see end of file):
 *   - /admin/sharing/reports     — needs a backend reports table that
 *                                  doesn't exist yet.
 *   - User detail sub-tab on /admin/users/[id]
 *   - Org detail sub-tab on /admin/orgs/[slug]
 *
 * Data source for v1:
 *   No admin-wide "active shares per user" endpoint exists today. The
 *   caller-scoped helpers (`listShares`, `listSharedWithMe`) only see
 *   the bearer's own grants, which is useless for an admin overview.
 *
 *   As a workable proxy we tail the most recent N audit-log rows for
 *   the sharing-related actions and surface:
 *     - rolling-window grant / revoke / scope-change / report / view
 *       counters (last 500 rows)
 *     - the top-20 actors by share.created event count in that window
 *
 *   This is intentionally a "window" view, not a true active-share
 *   inventory. A future backend endpoint
 *   (e.g. GET /v1/admin/sharing/overview) should return real counts
 *   off the `share_with_user` / `share_with_org` tables. Until then
 *   the eyebrow on each card says "recent" so admins don't mistake
 *   the proxy for ground truth.
 *
 * Auth: parent layout (`/admin/layout.tsx`) gates the whole subtree
 * on moderator/admin role. We still call `getSession()` here for type
 * narrowing + a defensive redirect.
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getAdminAuditLog,
  type AuditEntryDto,
} from '@/lib/api';
import { getSession } from '@/lib/session';
import { AdminNav } from '../_components/AdminNav';

/**
 * Audit-log action names we treat as "sharing activity". The backend
 * (W3-S) is adding `share.viewed` + `share.scope_changed` + the
 * report variant during this wave; we list them up front so the
 * overview keeps working as soon as those rows start landing.
 *
 * Keep this list in sync with the audit page below.
 */
const SHARING_ACTIONS = [
  'share.created',
  'share.revoked',
  'share.scope_changed',
  'share.viewed',
  'share.reported',
] as const;

/**
 * How many recent rows we pull to compute the proxy stats. Keep
 * generous enough that low-traffic moments still show something, but
 * small enough that one server fetch round-trip stays snappy. The
 * limit is shared across all five action filters (five parallel
 * fetches — see TODO about a single overview endpoint).
 */
const WINDOW_SIZE = 500;

interface OverviewBuckets {
  created: ReadonlyArray<AuditEntryDto>;
  revoked: ReadonlyArray<AuditEntryDto>;
  scopeChanged: ReadonlyArray<AuditEntryDto>;
  viewed: ReadonlyArray<AuditEntryDto>;
  reported: ReadonlyArray<AuditEntryDto>;
}

export default async function AdminSharingOverviewPage() {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/admin/sharing');

  // The audit endpoint takes a single `action` filter — no IN-clause
  // support today. Issue five parallel fetches so a busy actor on
  // share.created doesn't crowd out share.revoked rows in the same
  // window.
  //
  // TODO(backend): a dedicated GET /v1/admin/sharing/overview that
  // returns precomputed counts off the underlying tables would
  // replace these five round-trips with one and would give us *true*
  // active-share counts (this view only sees the most recent
  // WINDOW_SIZE events).
  let buckets: OverviewBuckets;
  try {
    const fetched = await Promise.all(
      SHARING_ACTIONS.map((action) =>
        getAdminAuditLog(session.token, {
          action,
          limit: WINDOW_SIZE,
        }).then((r) => r.entries),
      ),
    );
    buckets = {
      created: fetched[0] ?? [],
      revoked: fetched[1] ?? [],
      scopeChanged: fetched[2] ?? [],
      viewed: fetched[3] ?? [],
      reported: fetched[4] ?? [],
    };
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/admin/sharing');
    }
    if (e instanceof ApiCallError && e.status === 403) {
      redirect('/dashboard');
    }
    throw e;
  }

  const totalEvents =
    buckets.created.length +
    buckets.revoked.length +
    buckets.scopeChanged.length +
    buckets.viewed.length +
    buckets.reported.length;

  // Net grants = created - revoked across the window. Negative when
  // revoke storms outpace grants. Useful "is something on fire"
  // signal even though it isn't a true active-share total.
  const netGrants = buckets.created.length - buckets.revoked.length;

  // "Top users by share.created" — only counts events in the window.
  // A future endpoint should join against share_with_user for the
  // *current* active count instead. Rendered in a compact table to
  // match the rest of the admin surface.
  const topGranters = rankByActor(buckets.created).slice(0, 20);

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <AdminNav current="sharing" />

      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Admin · sharing
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Sharing overview
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
            maxWidth: 640,
          }}
        >
          Rolling window of the last {WINDOW_SIZE} events per action,
          read off the audit log. Treat the numbers as recent activity,
          not a live active-share inventory — that view is on the
          backend backlog.
        </p>
      </header>

      <section
        aria-label="Recent sharing activity"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
          gap: 12,
        }}
      >
        <StatCard
          eyebrow="Outbound grants"
          label="share.created"
          value={buckets.created.length}
          hint={`Net grants: ${netGrants >= 0 ? '+' : ''}${netGrants}`}
          tone={netGrants >= 0 ? 'ok' : 'danger'}
        />
        <StatCard
          eyebrow="Revocations"
          label="share.revoked"
          value={buckets.revoked.length}
        />
        <StatCard
          eyebrow="Scope changes"
          label="share.scope_changed"
          value={buckets.scopeChanged.length}
          hint="Per-share scope edits (W3-S backend)"
        />
        <StatCard
          eyebrow="Views"
          label="share.viewed"
          value={buckets.viewed.length}
          hint="Recipient reads (W3-S backend)"
        />
        <StatCard
          eyebrow="Reports"
          label="share.reported"
          value={buckets.reported.length}
          tone={buckets.reported.length > 0 ? 'danger' : undefined}
          hint="Triage queue lives at /admin/sharing/reports"
        />
      </section>

      <section className="ss-card" style={{ padding: '20px 24px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Top granters (recent window)
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          Most-active sharers
        </h2>
        <p
          style={{
            margin: '8px 0 16px',
            color: 'var(--fg-muted)',
            fontSize: 13,
            lineHeight: 1.5,
          }}
        >
          Counts share.created events per actor across the last{' '}
          {WINDOW_SIZE} rows. A user who shared then revoked still
          counts here — this is a proxy for "who's most active", not
          "who currently shares the most".
        </p>
        {topGranters.length === 0 ? (
          <p
            style={{
              margin: 0,
              padding: '20px 0',
              color: 'var(--fg-muted)',
              fontSize: 13,
            }}
          >
            Scope is clear — no share.created events in the recent
            window.
          </p>
        ) : (
          <table
            style={{
              width: '100%',
              borderCollapse: 'collapse',
              fontSize: 13,
            }}
          >
            <thead>
              <tr style={{ background: 'var(--bg-elev)' }}>
                <Th width="48px">#</Th>
                <Th>Handle</Th>
                <Th width="120px">Grants</Th>
              </tr>
            </thead>
            <tbody>
              {topGranters.map((row, i) => (
                <tr
                  key={row.handle}
                  style={{ borderBottom: '1px solid var(--border)' }}
                >
                  <Td>
                    <span
                      className="mono"
                      style={{ color: 'var(--fg-dim)' }}
                    >
                      {i + 1}
                    </span>
                  </Td>
                  <Td>
                    <Link
                      href={
                        (`/u/${encodeURIComponent(row.handle)}`) as Route
                      }
                      className="mono"
                      style={{ color: 'var(--accent)' }}
                    >
                      {row.handle}
                    </Link>
                  </Td>
                  <Td>
                    <span className="mono">{row.count}</span>
                  </Td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section className="ss-card" style={{ padding: '20px 24px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Scope distributions
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          Common share scopes
        </h2>
        <p
          style={{
            margin: '8px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 13,
            lineHeight: 1.5,
          }}
        >
          Placeholder. Once the per-share scope columns land on the
          backend (W3-S), this card will surface a histogram of which
          scope masks are in use across active grants. Today the audit
          payload carries the scope, but aggregating across
          share.created vs share.scope_changed (with revoke
          subtraction) for an accurate "active scope" tally requires
          a dedicated query that doesn't exist yet.
        </p>
        <p
          style={{
            margin: '12px 0 0',
            color: 'var(--fg-dim)',
            fontSize: 12,
          }}
        >
          TODO(backend): GET /v1/admin/sharing/scope-histogram.
        </p>
      </section>

      <nav
        aria-label="Sharing sub-views"
        style={{
          display: 'flex',
          gap: 8,
          flexWrap: 'wrap',
        }}
      >
        <Link
          href={'/admin/sharing/audit' as Route}
          className="ss-btn ss-btn--ghost"
          style={{ textDecoration: 'none' }}
        >
          Sharing audit log →
        </Link>
      </nav>

      <p
        style={{
          margin: 0,
          color: 'var(--fg-dim)',
          fontSize: 11,
          lineHeight: 1.5,
        }}
      >
        Window stats above are computed from{' '}
        {totalEvents.toLocaleString()} recent audit rows. Older
        activity is not surfaced — use the audit log view for
        time-bounded queries.
      </p>
    </div>
  );
}

/**
 * Group entries by `actor_handle` and return descending counts.
 * Entries without an actor (system actions) are dropped — only real
 * users belong on a "top granters" leaderboard.
 *
 * Built immutably: we accumulate into a fresh `Map` rather than
 * mutating an existing structure.
 */
function rankByActor(
  entries: ReadonlyArray<AuditEntryDto>,
): ReadonlyArray<{ handle: string; count: number }> {
  const counts = new Map<string, number>();
  for (const e of entries) {
    if (!e.actor_handle) continue;
    counts.set(e.actor_handle, (counts.get(e.actor_handle) ?? 0) + 1);
  }
  return Array.from(counts.entries())
    .map(([handle, count]) => ({ handle, count }))
    .sort((a, b) => b.count - a.count);
}

function StatCard({
  eyebrow,
  label,
  value,
  hint,
  tone,
}: {
  eyebrow: string;
  label: string;
  value: number;
  hint?: string;
  tone?: 'ok' | 'danger';
}) {
  const accent =
    tone === 'danger'
      ? 'var(--danger)'
      : tone === 'ok'
        ? 'var(--accent)'
        : 'var(--fg)';
  return (
    <div
      className="ss-card"
      style={{
        padding: '16px 18px',
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div className="ss-eyebrow">{eyebrow}</div>
      <div
        style={{
          fontSize: 26,
          fontWeight: 600,
          letterSpacing: '-0.02em',
          color: accent,
        }}
      >
        {value.toLocaleString()}
      </div>
      <div
        className="mono"
        style={{ color: 'var(--fg-dim)', fontSize: 11 }}
      >
        {label}
      </div>
      {hint && (
        <div
          style={{
            color: 'var(--fg-muted)',
            fontSize: 11,
            lineHeight: 1.4,
            marginTop: 2,
          }}
        >
          {hint}
        </div>
      )}
    </div>
  );
}

function Th({
  children,
  width,
}: {
  children: React.ReactNode;
  width?: string;
}) {
  return (
    <th
      style={{
        textAlign: 'left',
        padding: '10px 14px',
        fontWeight: 600,
        color: 'var(--fg-muted)',
        fontSize: 11,
        letterSpacing: '0.06em',
        textTransform: 'uppercase',
        borderBottom: '1px solid var(--border)',
        width,
      }}
    >
      {children}
    </th>
  );
}

function Td({ children }: { children: React.ReactNode }) {
  return (
    <td style={{ padding: '10px 14px', verticalAlign: 'top' }}>
      {children}
    </td>
  );
}

// TODO(W3 follow-up wave): three more views from audit §05.
//
//   1. /admin/sharing/reports
//      Triage queue for `share.reported` events. Needs a backend
//      `share_reports` table (status: open/acknowledged/resolved,
//      reporter_id, share_id, reason, resolved_by, resolved_at).
//      No backend table exists today — W3-S scope is per-share
//      scopes + share.viewed, not the reports queue.
//
//   2. User detail sub-tab on /admin/users/[id]/page.tsx
//      Add a "Sharing" tab alongside the existing role-grant UI. It
//      should list (a) the user's outbound grants, (b) their inbound
//      grants, (c) sharing-related audit rows scoped to actor_handle
//      = this user. Needs a `/v1/admin/users/{id}/shares` endpoint
//      (or two existing ones called with admin impersonation).
//
//   3. Org detail sub-tab on /admin/orgs/[slug]/page.tsx
//      List members who've granted the org access (org_shares)
//      plus a per-org slice of share.created/revoked events.
//
//   Per W3 scope these are noted in the audit report but not built.
//   See the report for cross-references to the exact files where
//   the sub-tabs should slot in.

// Force dynamic render: the audit log changes on every state-
// changing API call, so a static cache would lie within seconds.
export const dynamic = 'force-dynamic';
