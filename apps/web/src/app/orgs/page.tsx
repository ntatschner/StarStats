import Link from 'next/link';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  listOrgs,
  type ListOrgsResponse,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';

interface SearchParams {
  status?: string;
  error?: string;
}

const mainStyle: React.CSSProperties = {
  maxWidth: 'none',
  margin: 0,
  padding: 0,
  display: 'flex',
  flexDirection: 'column',
  gap: 20,
};

const headerTitleStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 32,
  fontWeight: 600,
  letterSpacing: '-0.02em',
};

const headerSubtitleStyle: React.CSSProperties = {
  margin: '6px 0 0',
  color: 'var(--fg-muted)',
  fontSize: 14,
};

const ctaRowStyle: React.CSSProperties = {
  display: 'flex',
  gap: 12,
  flexWrap: 'wrap',
};

const orgGridStyle: React.CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
  gap: 16,
};

const orgCardStyle: React.CSSProperties = {
  padding: '20px 22px',
  display: 'flex',
  flexDirection: 'column',
  gap: 12,
};

const orgCardHeadStyle: React.CSSProperties = {
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'flex-start',
  gap: 12,
};

const orgNameStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 16,
  fontWeight: 600,
  letterSpacing: '-0.01em',
};

const orgSlugStyle: React.CSSProperties = {
  color: 'var(--fg-dim)',
  fontSize: 12,
};

const orgFootStyle: React.CSSProperties = {
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'center',
};

const emptyStyle: React.CSSProperties = {
  textAlign: 'center',
  padding: '40px 20px',
  color: 'var(--fg-muted)',
  fontSize: 14,
};

const emptyTitleStyle: React.CSSProperties = {
  fontSize: 16,
  color: 'var(--fg)',
  marginBottom: 6,
};

export default async function OrgsPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/orgs');

  const { status, error } = await props.searchParams;

  let orgs: ListOrgsResponse = { orgs: [] };
  let degraded = false;
  try {
    orgs = await listOrgs(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/orgs');
    }
    if (e instanceof ApiCallError && e.status === 503) {
      degraded = true;
    } else {
      logger.error({ err: e }, 'list orgs failed');
      degraded = true;
    }
  }

  return (
    <main className="ss-screen-enter" style={mainStyle}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Orgs
        </div>
        <h1 style={headerTitleStyle}>Your orgs</h1>
        <p style={headerSubtitleStyle}>
          Orgs are loose groupings — a slug, a name, a list of members. Share
          your manifest with an org and every member can read it.
        </p>
      </header>

      {status && (
        <div className="ss-alert ss-alert--ok" role="status">
          {labelForStatus(status)}
        </div>
      )}
      {error && (
        <div className="ss-alert ss-alert--danger" role="alert">
          {labelForError(error)}
        </div>
      )}

      <div style={ctaRowStyle}>
        <Link href="/orgs/new" className="ss-btn ss-btn--primary">
          Create org
        </Link>
      </div>

      {degraded ? (
        <section className="ss-card ss-card-pad">
          <div style={emptyStyle}>
            <div style={emptyTitleStyle}>Comms down.</div>
            <div>Organizations are temporarily unavailable. Try again shortly.</div>
          </div>
        </section>
      ) : orgs.orgs.length === 0 ? (
        <section className="ss-card ss-card-pad">
          <div style={emptyStyle}>
            <div style={emptyTitleStyle}>Scope is clear.</div>
            <div>
              You don&apos;t own any orgs yet. Create one to share your
              manifest with a group.
            </div>
          </div>
        </section>
      ) : (
        <div style={orgGridStyle}>
          {orgs.orgs.map((o) => (
            <div key={o.id} className="ss-card" style={orgCardStyle}>
              <div style={orgCardHeadStyle}>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  <h3 style={orgNameStyle}>{o.name}</h3>
                  <span className="mono" style={orgSlugStyle}>
                    /orgs/{o.slug}
                  </span>
                </div>
                <span className="ss-badge ss-badge--accent">Owner</span>
              </div>
              <div style={orgFootStyle}>
                <span style={{ color: 'var(--fg-muted)', fontSize: 13 }}>
                  Owned by you
                </span>
                <Link
                  href={`/orgs/${encodeURIComponent(o.slug)}`}
                  className="ss-btn ss-btn--link"
                >
                  Open →
                </Link>
              </div>
            </div>
          ))}
        </div>
      )}
    </main>
  );
}

function labelForStatus(code: string): string {
  switch (code) {
    case 'org_created':
      return 'Organization created.';
    case 'org_deleted':
      return 'Organization deleted.';
    default:
      return 'Done.';
  }
}

function labelForError(code: string): string {
  switch (code) {
    case 'slug_collision':
      return "We couldn't generate a unique URL for that name. Try a different name.";
    case 'invalid_name':
      return 'That name is empty or has no usable characters.';
    case 'spicedb_unavailable':
      return 'Organizations are temporarily unavailable. Try again shortly.';
    case 'unexpected':
      return 'Something went wrong. Please try again.';
    default:
      return `Couldn't complete that action (${code}).`;
  }
}
