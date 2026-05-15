/**
 * Admin · Org detail.
 *
 * Surfaces the AdminOrgDto + a force-delete confirmation form. The
 * confirmation requires typing the slug exactly to prevent fat-finger
 * deletes — same posture as the owner-facing delete in /orgs/[slug].
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  deleteAdminOrg,
  getAdminOrg,
  type AdminOrgDto,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';
import { AdminNav } from '../../_components/AdminNav';

interface PageProps {
  params: Promise<{ slug: string }>;
  searchParams: Promise<{ error?: string }>;
}

const ERROR_MESSAGES: Record<string, string> = {
  slug_mismatch:
    "Slug confirmation didn't match. Type the slug exactly to confirm.",
  forbidden: 'Admin role required.',
  org_not_found: 'Org no longer exists.',
  unexpected: 'Something went wrong. Try again.',
};

export default async function AdminOrgDetailPage(props: PageProps) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/admin/orgs');

  const { slug } = await props.params;
  const params = await props.searchParams;
  const errorCode = params.error;

  let org: AdminOrgDto;
  try {
    org = await getAdminOrg(session.token, slug);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/admin/orgs');
    }
    if (e instanceof ApiCallError && e.status === 403) {
      redirect('/dashboard');
    }
    if (e instanceof ApiCallError && e.status === 404) {
      redirect('/admin/orgs?error=org_not_found');
    }
    throw e;
  }

  const isAdmin = session.staffRoles.some((r) => r === 'admin');

  async function deleteAction(formData: FormData) {
    'use server';
    const s = await getSession();
    if (!s) redirect('/auth/login?next=/admin/orgs');
    const confirm = String(formData.get('confirm') ?? '').trim();
    if (confirm !== slug) {
      redirect(`/admin/orgs/${encodeURIComponent(slug)}?error=slug_mismatch`);
    }
    try {
      await deleteAdminOrg(s.token, slug);
      redirect('/admin/orgs?status=org_deleted');
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/admin/orgs');
        if (e.status === 403)
          redirect(`/admin/orgs/${encodeURIComponent(slug)}?error=forbidden`);
        if (e.status === 404) redirect('/admin/orgs?error=org_not_found');
      }
      logger.error({ err: e }, 'admin org delete failed');
      redirect(`/admin/orgs/${encodeURIComponent(slug)}?error=unexpected`);
    }
  }

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <AdminNav current="orgs" />

      <Link
        href={'/admin/orgs' as Route}
        style={{ fontSize: 13, color: 'var(--accent)', textDecoration: 'none' }}
      >
        ← All orgs
      </Link>

      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Admin · org detail
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          {org.name}
        </h1>
        <p
          style={{ margin: '6px 0 0', color: 'var(--fg-muted)', fontSize: 13 }}
        >
          <span className="mono">{org.slug}</span>
        </p>
      </header>

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
          Org info
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
            <span className="mono">{org.id}</span>
          </Dd>
          <Dt>Slug</Dt>
          <Dd>
            <span className="mono">{org.slug}</span>
          </Dd>
          <Dt>Owner user</Dt>
          <Dd>
            <span className="mono">{org.owner_user_id}</span>
          </Dd>
          <Dt>Created</Dt>
          <Dd>
            <span className="mono">{org.created_at}</span>
          </Dd>
          <Dt>Members</Dt>
          <Dd>{org.member_count}</Dd>
        </dl>
      </section>

      <section
        className="ss-card"
        style={{ padding: '20px 24px', borderColor: 'var(--danger)' }}
      >
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Danger zone
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            color: 'var(--danger)',
          }}
        >
          Force-delete org
        </h2>
        <p
          style={{
            margin: '10px 0 14px',
            color: 'var(--fg-muted)',
            fontSize: 13,
          }}
        >
          Wipes the Postgres row AND every SpiceDB relationship
          (members, owner, admins, share-with-org grants). The audit
          log keeps a record. To confirm, type{' '}
          <span className="mono" style={{ color: 'var(--fg)' }}>
            {org.slug}
          </span>{' '}
          into the field below.
        </p>
        {!isAdmin ? (
          <p style={{ color: 'var(--fg-muted)', fontSize: 13 }}>
            Admin role required.
          </p>
        ) : (
          <form
            action={deleteAction}
            style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}
          >
            <input
              type="text"
              name="confirm"
              placeholder={org.slug}
              required
              autoComplete="off"
              spellCheck={false}
              className="mono"
              style={{
                flex: '1 1 240px',
                padding: '8px 12px',
                background: 'var(--bg-elev)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--r-sm)',
                color: 'var(--fg)',
              }}
            />
            <button
              type="submit"
              className="ss-btn ss-btn--ghost"
              style={{ color: 'var(--danger)', borderColor: 'var(--danger)' }}
            >
              Delete org
            </button>
          </form>
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
