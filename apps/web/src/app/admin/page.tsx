/**
 * Admin landing page — moderator-facing dashboard.
 *
 * Auth: the `/admin/layout.tsx` (D1 owns) gates the whole subtree on
 * moderator/admin role. We still call `getSession()` here for type
 * narrowing and a defensive redirect — the layout's redirect happens
 * first but a no-session render path would otherwise be a type error
 * when reading `session.token`.
 *
 * The two queue cards probe `getAdminSubmissionQueue` with `limit=1`
 * for each status bucket. Mirrors the probe pattern from
 * `app/submissions/page.tsx` (lines 71-100). We deliberately don't
 * surface counts — the API has no `total` yet — only "empty / not".
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getAdminSubmissionQueue,
} from '@/lib/api';
import { getSession } from '@/lib/session';
import { AdminNav } from './_components/AdminNav';

export default async function AdminLandingPage() {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/admin');

  let probeReview = false;
  let probeFlagged = false;
  try {
    const [review, flagged] = await Promise.all([
      getAdminSubmissionQueue(session.token, { status: 'review', limit: 1 }),
      getAdminSubmissionQueue(session.token, { status: 'flagged', limit: 1 }),
    ]);
    // `has_more` alone isn't enough — a bucket with exactly 1 item has
    // `has_more: false`. Treat "non-empty" as either flag.
    probeReview = review.items.length > 0 || review.has_more;
    probeFlagged = flagged.items.length > 0 || flagged.has_more;
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/admin');
    }
    if (e instanceof ApiCallError && e.status === 403) {
      // The layout should have caught this, but surface a redirect
      // rather than a 500 if a non-mod somehow reached the page.
      redirect('/dashboard');
    }
    throw e;
  }

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <AdminNav current="dashboard" />

      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Admin · moderation console
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Moderation
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
            maxWidth: 640,
          }}
        >
          Triage community-submitted parser rules. Accept ships them in
          the next parser update; reject sends a reason back to the
          submitter; dismiss-flag clears community reports without
          changing rule status.
        </p>
      </header>

      <div
        data-rspgrid="2"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
          gap: 16,
        }}
      >
        <QueueCard
          eyebrow="Triage queue"
          title="Submissions in review"
          description="Newly proposed parser rules awaiting a moderator decision."
          href={'/admin/submissions?status=review' as Route}
          nonEmpty={probeReview}
        />
        <QueueCard
          eyebrow="Community reports"
          title="Flagged submissions"
          description="Already-accepted patterns that the community has flagged for revisiting."
          href={'/admin/submissions?status=flagged' as Route}
          nonEmpty={probeFlagged}
        />
      </div>

      <section className="ss-card" style={{ padding: '20px 24px' }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
          Recent admin actions
        </div>
        <h2
          style={{
            margin: 0,
            fontSize: 17,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          Open the audit log
        </h2>
        <p
          style={{
            margin: '10px 0 16px',
            color: 'var(--fg-muted)',
            fontSize: 13,
            lineHeight: 1.6,
          }}
        >
          Every state-changing API call writes one hash-chained row. The
          viewer is filterable by actor, action, and timestamp range.
        </p>
        <Link
          href={'/admin/audit' as Route}
          className="ss-btn ss-btn--ghost"
          style={{ textDecoration: 'none' }}
        >
          Audit log →
        </Link>
      </section>
    </div>
  );
}

function QueueCard({
  eyebrow,
  title,
  description,
  href,
  nonEmpty,
}: {
  eyebrow: string;
  title: string;
  description: string;
  href: Route;
  nonEmpty: boolean;
}) {
  return (
    <Link
      href={href}
      className="ss-card"
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        padding: '20px 22px',
        textDecoration: 'none',
        color: 'inherit',
        minHeight: 160,
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
        }}
      >
        <div className="ss-eyebrow">{eyebrow}</div>
        <span
          aria-label={nonEmpty ? 'Has pending items' : 'Empty'}
          title={nonEmpty ? 'Has pending items' : 'Empty'}
          style={{
            width: 8,
            height: 8,
            borderRadius: 999,
            background: nonEmpty ? 'var(--accent)' : 'var(--border-strong)',
          }}
        />
      </div>
      <h2
        style={{
          margin: 0,
          fontSize: 18,
          fontWeight: 600,
          letterSpacing: '-0.01em',
        }}
      >
        {title}
      </h2>
      <p
        style={{
          margin: 0,
          color: 'var(--fg-muted)',
          fontSize: 13,
          lineHeight: 1.5,
        }}
      >
        {description}
      </p>
      <div
        style={{
          marginTop: 'auto',
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          color: nonEmpty ? 'var(--accent)' : 'var(--fg-dim)',
          fontSize: 13,
        }}
      >
        <span>{nonEmpty ? 'Open queue' : 'Nothing waiting'}</span>
        <span aria-hidden="true">→</span>
      </div>
    </Link>
  );
}
