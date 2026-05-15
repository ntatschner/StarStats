/**
 * Admin · Users.
 *
 * Paginated list with substring search over claimed_handle / email.
 * Surfaces verification + staff-role badges so admins can scan for
 * unverified or escalated accounts at a glance. Clicking a row drills
 * into /admin/users/[id] for role grants/revokes.
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getAdminUsers,
  type AdminUserDto,
  type AdminUserListResponse,
} from '@/lib/api';
import { getSession } from '@/lib/session';
import { AdminNav } from '../_components/AdminNav';

interface SearchParams {
  q?: string;
  offset?: string;
}

const PAGE_SIZE = 50;

export default async function AdminUsersPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/admin/users');

  const params = await props.searchParams;
  const q = params.q?.trim() ?? '';
  const offset = parsePositiveInt(params.offset);

  let result: AdminUserListResponse;
  try {
    result = await getAdminUsers(session.token, {
      q: q || undefined,
      limit: PAGE_SIZE,
      offset,
    });
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/admin/users');
    }
    if (e instanceof ApiCallError && e.status === 403) {
      redirect('/dashboard');
    }
    throw e;
  }

  const buildHref = (newOffset: number): Route => {
    const qs = new URLSearchParams();
    if (q) qs.set('q', q);
    if (newOffset > 0) qs.set('offset', String(newOffset));
    const s = qs.toString();
    return (s ? `/admin/users?${s}` : '/admin/users') as Route;
  };

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <AdminNav current="users" />

      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Admin · users
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Users
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
            maxWidth: 640,
          }}
        >
          Search by handle or email. Row links open the detail page where
          moderator + admin role grants live (admin role required for
          mutation, moderator role required for read).
        </p>
      </header>

      <form
        method="GET"
        action="/admin/users"
        style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}
      >
        <input
          type="search"
          name="q"
          defaultValue={q}
          placeholder="Search handles or emails…"
          autoComplete="off"
          spellCheck={false}
          className="mono"
          style={{
            flex: '1 1 260px',
            padding: '8px 12px',
            background: 'var(--bg-elev)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--r-sm)',
            color: 'var(--fg)',
          }}
        />
        <button type="submit" className="ss-btn ss-btn--primary">
          Search
        </button>
        {q && (
          <Link
            href={'/admin/users' as Route}
            className="ss-btn ss-btn--ghost"
            style={{ textDecoration: 'none' }}
          >
            Clear
          </Link>
        )}
      </form>

      <section className="ss-card" style={{ padding: 0, overflow: 'hidden' }}>
        {result.users.length === 0 ? (
          <p
            style={{
              margin: 0,
              padding: '40px 24px',
              textAlign: 'center',
              color: 'var(--fg-muted)',
              fontSize: 14,
            }}
          >
            No users match this search.
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
                <Th>Handle</Th>
                <Th>Email</Th>
                <Th>Joined</Th>
                <Th>Status</Th>
                <Th>Roles</Th>
              </tr>
            </thead>
            <tbody>
              {result.users.map((u) => (
                <UserRow key={u.id} user={u} />
              ))}
            </tbody>
          </table>
        )}
      </section>

      <nav
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          gap: 12,
          flexWrap: 'wrap',
        }}
      >
        <span style={{ color: 'var(--fg-muted)', fontSize: 13 }}>
          {result.users.length === 0
            ? 'Nothing on this page'
            : `Showing ${result.users.length} users`}
        </span>
        <div style={{ display: 'flex', gap: 8 }}>
          {offset > 0 ? (
            <Link
              href={buildHref(Math.max(0, offset - PAGE_SIZE))}
              className="ss-btn ss-btn--ghost"
            >
              ← Newer
            </Link>
          ) : (
            <span
              className="ss-btn ss-btn--ghost"
              style={{ opacity: 0.4, pointerEvents: 'none' }}
            >
              ← Newer
            </span>
          )}
          {result.has_more ? (
            <Link
              href={buildHref(offset + PAGE_SIZE)}
              className="ss-btn ss-btn--ghost"
            >
              Older →
            </Link>
          ) : (
            <span
              className="ss-btn ss-btn--ghost"
              style={{ opacity: 0.4, pointerEvents: 'none' }}
            >
              Older →
            </span>
          )}
        </div>
      </nav>
    </div>
  );
}

function parsePositiveInt(raw: string | undefined): number {
  if (!raw) return 0;
  const n = Number.parseInt(raw, 10);
  if (!Number.isFinite(n) || n < 0) return 0;
  return n;
}

function Th({ children }: { children: React.ReactNode }) {
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
      }}
    >
      {children}
    </th>
  );
}

function UserRow({ user }: { user: AdminUserDto }) {
  const detailHref = (`/admin/users/${user.id}`) as Route;
  const joined = new Date(user.created_at).toISOString().slice(0, 10);
  return (
    <tr style={{ borderBottom: '1px solid var(--border)' }}>
      <td style={{ padding: '10px 14px' }}>
        <Link
          href={detailHref}
          className="mono"
          style={{ color: 'var(--accent)' }}
        >
          {user.claimed_handle}
        </Link>
      </td>
      <td style={{ padding: '10px 14px', color: 'var(--fg-muted)' }}>
        {user.email}
      </td>
      <td
        style={{
          padding: '10px 14px',
          color: 'var(--fg-muted)',
          fontSize: 12,
        }}
        className="mono"
      >
        {joined}
      </td>
      <td style={{ padding: '10px 14px' }}>
        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
          <StatusChip ok={user.email_verified} label="email" />
          <StatusChip ok={user.rsi_verified} label="RSI" />
          {user.totp_enabled && (
            <span className="ss-badge ss-badge--ok" style={{ fontSize: 10 }}>
              2FA
            </span>
          )}
        </div>
      </td>
      <td style={{ padding: '10px 14px' }}>
        {user.staff_roles.length === 0 ? (
          <span style={{ color: 'var(--fg-dim)', fontSize: 12 }}>—</span>
        ) : (
          <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
            {user.staff_roles.map((r) => (
              <span
                key={r}
                className="ss-badge"
                style={{
                  fontSize: 10,
                  color: 'var(--accent)',
                  borderColor: 'var(--accent)',
                }}
              >
                {r}
              </span>
            ))}
          </div>
        )}
      </td>
    </tr>
  );
}

function StatusChip({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span
      className={ok ? 'ss-badge ss-badge--ok' : 'ss-badge'}
      style={{
        fontSize: 10,
        color: ok ? undefined : 'var(--fg-dim)',
        borderColor: ok ? undefined : 'var(--border)',
      }}
    >
      {ok ? `✓ ${label}` : `· ${label}`}
    </span>
  );
}
