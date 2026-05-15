/**
 * Admin · User detail.
 *
 * Surfaces the AdminUserDto + provides role grant/revoke forms.
 * Mutations use Server Actions that hit POST/DELETE on
 * /v1/admin/users/:id/roles. Status feedback flows back through the
 * URL so the page works without client JS.
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getAdminUser,
  grantAdminUserRole,
  revokeAdminUserRole,
  type AdminUserDto,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';
import { AdminNav } from '../../_components/AdminNav';

interface PageProps {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ status?: string; error?: string }>;
}

const STATUS_MESSAGES: Record<string, string> = {
  role_granted: 'Role granted.',
  role_revoked: 'Role revoked.',
  no_change: 'No change — the user already had that role state.',
};

const ERROR_MESSAGES: Record<string, string> = {
  invalid_role: 'Invalid role. Pick moderator or admin.',
  reason_too_long: 'Reason too long (max 280 characters).',
  user_not_found: 'User no longer exists.',
  cannot_revoke_own_admin: "You can't revoke your own admin role.",
  forbidden: 'Admin role required for that change.',
  unexpected: 'Something went wrong. Try again.',
};

export default async function AdminUserDetailPage(props: PageProps) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/admin/users');

  const { id } = await props.params;
  const params = await props.searchParams;
  const status = params.status;
  const errorCode = params.error;

  let user: AdminUserDto;
  try {
    user = await getAdminUser(session.token, id);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/admin/users');
    }
    if (e instanceof ApiCallError && e.status === 403) {
      redirect('/dashboard');
    }
    if (e instanceof ApiCallError && e.status === 404) {
      redirect('/admin/users?error=user_not_found');
    }
    throw e;
  }

  const isAdmin = session.staffRoles.some((r) => r === 'admin');
  const isModerator = user.staff_roles.includes('moderator');
  const isAdminTarget = user.staff_roles.includes('admin');
  const isSelf =
    session.claimedHandle.toLowerCase() ===
    user.claimed_handle.toLowerCase();

  async function grantAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/admin/users');
    const role = String(formData.get('role') ?? '').trim();
    const reason =
      String(formData.get('reason') ?? '').trim() || undefined;
    try {
      const res = await grantAdminUserRole(s.token, id, { role, reason });
      redirect(
        `/admin/users/${id}?status=${res.changed ? 'role_granted' : 'no_change'}`,
      );
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/admin/users');
        if (e.status === 403) redirect(`/admin/users/${id}?error=forbidden`);
        if (e.status === 404) redirect(`/admin/users?error=user_not_found`);
        if (e.status === 400)
          redirect(
            `/admin/users/${id}?error=${encodeURIComponent(e.body.error)}`,
          );
      }
      logger.error({ err: e }, 'admin grant role failed');
      redirect(`/admin/users/${id}?error=unexpected`);
    }
  }

  async function revokeAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/admin/users');
    const role = String(formData.get('role') ?? '') as 'moderator' | 'admin';
    if (role !== 'moderator' && role !== 'admin') {
      redirect(`/admin/users/${id}?error=invalid_role`);
    }
    try {
      const res = await revokeAdminUserRole(s.token, id, role);
      redirect(
        `/admin/users/${id}?status=${res.changed ? 'role_revoked' : 'no_change'}`,
      );
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/admin/users');
        if (e.status === 403) redirect(`/admin/users/${id}?error=forbidden`);
        if (e.status === 404) redirect(`/admin/users?error=user_not_found`);
        if (e.status === 400)
          redirect(
            `/admin/users/${id}?error=${encodeURIComponent(e.body.error)}`,
          );
      }
      logger.error({ err: e }, 'admin revoke role failed');
      redirect(`/admin/users/${id}?error=unexpected`);
    }
  }

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <AdminNav current="users" />

      <Link
        href={'/admin/users' as Route}
        style={{ fontSize: 13, color: 'var(--accent)', textDecoration: 'none' }}
      >
        ← All users
      </Link>

      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Admin · user detail
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
          className="mono"
        >
          {user.claimed_handle}
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 13,
          }}
        >
          {user.email}
          {isSelf && (
            <span style={{ marginLeft: 8, color: 'var(--accent)' }}>(you)</span>
          )}
        </p>
      </header>

      {status && STATUS_MESSAGES[status] && (
        <div className="ss-badge ss-badge--ok" style={{ alignSelf: 'flex-start' }}>
          {STATUS_MESSAGES[status]}
        </div>
      )}
      {errorCode && (
        <div
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

      <section className="ss-card" style={{ padding: '20px 24px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Account
        </div>
        <dl
          style={{
            display: 'grid',
            gridTemplateColumns: 'auto 1fr',
            gap: '8px 16px',
            margin: '10px 0 0',
            fontSize: 13,
          }}
        >
          <Dt>UUID</Dt>
          <Dd>
            <span className="mono">{user.id}</span>
          </Dd>
          <Dt>Joined</Dt>
          <Dd>
            <span className="mono">{user.created_at}</span>
          </Dd>
          <Dt>Email verified</Dt>
          <Dd>{user.email_verified ? '✓ yes' : '· no'}</Dd>
          <Dt>RSI verified</Dt>
          <Dd>{user.rsi_verified ? '✓ yes' : '· no'}</Dd>
          <Dt>2FA enabled</Dt>
          <Dd>{user.totp_enabled ? '✓ yes' : '· no'}</Dd>
          <Dt>Staff roles</Dt>
          <Dd>
            {user.staff_roles.length === 0 ? '—' : user.staff_roles.join(', ')}
          </Dd>
        </dl>
      </section>

      <section className="ss-card" style={{ padding: '20px 24px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Staff roles
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          Grant / revoke
        </h2>
        {!isAdmin ? (
          <p
            style={{
              margin: '12px 0 0',
              color: 'var(--fg-muted)',
              fontSize: 13,
            }}
          >
            Read-only view. Granting and revoking staff roles requires the
            admin role on your own account.
          </p>
        ) : (
          <div
            style={{
              marginTop: 14,
              display: 'flex',
              flexDirection: 'column',
              gap: 14,
            }}
          >
            <RoleControl
              role="moderator"
              active={isModerator}
              grantAction={grantAction}
              revokeAction={revokeAction}
              disableReason={null}
            />
            <RoleControl
              role="admin"
              active={isAdminTarget}
              grantAction={grantAction}
              revokeAction={revokeAction}
              disableReason={
                isSelf && isAdminTarget
                  ? "You can't revoke your own admin role."
                  : null
              }
            />
          </div>
        )}
      </section>
    </div>
  );
}

function Dt({ children }: { children: React.ReactNode }) {
  return (
    <dt
      style={{
        color: 'var(--fg-muted)',
        fontSize: 11,
        textTransform: 'uppercase',
        letterSpacing: '0.06em',
        alignSelf: 'center',
      }}
    >
      {children}
    </dt>
  );
}
function Dd({ children }: { children: React.ReactNode }) {
  return <dd style={{ margin: 0 }}>{children}</dd>;
}

function RoleControl({
  role,
  active,
  grantAction,
  revokeAction,
  disableReason,
}: {
  role: 'moderator' | 'admin';
  active: boolean;
  grantAction: (formData: FormData) => Promise<void>;
  revokeAction: (formData: FormData) => Promise<void>;
  disableReason: string | null;
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        justifyContent: 'space-between',
        gap: 16,
        padding: '12px 16px',
        background: 'var(--bg-elev)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--r-sm)',
        flexWrap: 'wrap',
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <span style={{ fontWeight: 600, fontSize: 14 }}>{role}</span>
        <span style={{ color: 'var(--fg-muted)', fontSize: 12 }}>
          {active ? 'Active grant' : 'Not granted'}
        </span>
      </div>
      {disableReason ? (
        <span
          style={{
            color: 'var(--fg-dim)',
            fontSize: 12,
            maxWidth: 280,
            textAlign: 'right',
          }}
        >
          {disableReason}
        </span>
      ) : active ? (
        <form action={revokeAction} style={{ margin: 0 }}>
          <input type="hidden" name="role" value={role} />
          <button
            type="submit"
            className="ss-btn ss-btn--ghost"
            style={{ color: 'var(--danger)' }}
          >
            Revoke {role}
          </button>
        </form>
      ) : (
        <form
          action={grantAction}
          style={{ margin: 0, display: 'flex', gap: 8 }}
        >
          <input type="hidden" name="role" value={role} />
          <input
            type="text"
            name="reason"
            placeholder="Reason (optional)"
            maxLength={280}
            style={{
              padding: '6px 10px',
              background: 'var(--bg)',
              border: '1px solid var(--border)',
              borderRadius: 'var(--r-sm)',
              color: 'var(--fg)',
              fontSize: 12,
              minWidth: 200,
            }}
          />
          <button type="submit" className="ss-btn ss-btn--primary">
            Grant {role}
          </button>
        </form>
      )}
    </div>
  );
}
