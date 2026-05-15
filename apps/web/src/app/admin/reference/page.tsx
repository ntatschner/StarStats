/**
 * Admin · Reference data summary.
 *
 * Surfaces the wiki-sync output (reference_registry) at category
 * granularity: how many rows each category holds, when the cron
 * last touched it. Drill into a category to see the rows.
 *
 * No write surface — refreshes live in the in-tree cron; this page
 * is strictly diagnostic ("is the location sync stuck?" / "did the
 * weapons table grow this month?").
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getAdminReferenceCategories,
  type AdminReferenceCategoryDto,
} from '@/lib/api';
import { getSession } from '@/lib/session';
import { AdminNav } from '../_components/AdminNav';

const CATEGORY_LABEL: Record<string, string> = {
  vehicle: 'Vehicles',
  weapon: 'Weapons',
  item: 'Items',
  location: 'Locations',
};

const CATEGORY_BLURB: Record<string, string> = {
  vehicle: 'Ships and ground vehicles — the canonical list of class names.',
  weapon: 'FPS + ship-mounted weapons.',
  item: 'Loose items: components, consumables, gear, attachments.',
  location: 'Star systems, planets, moons, stations, jump points.',
};

export default async function AdminReferencePage() {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/admin/reference');

  let data;
  try {
    data = await getAdminReferenceCategories(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/admin/reference');
    }
    if (e instanceof ApiCallError && e.status === 403) {
      redirect('/dashboard');
    }
    throw e;
  }

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <AdminNav current="reference" />

      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Admin · reference data
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Reference data
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
            maxWidth: 640,
          }}
        >
          The wiki-sync cron writes here daily. If a category hasn't
          been touched in a while, the sync is probably broken
          upstream — check the server logs.
        </p>
      </header>

      <section
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
          gap: 12,
        }}
      >
        {data.categories.map((c) => (
          <CategoryCard key={c.category} category={c} />
        ))}
      </section>
    </div>
  );
}

function CategoryCard({ category }: { category: AdminReferenceCategoryDto }) {
  const label = CATEGORY_LABEL[category.category] ?? category.category;
  const blurb = CATEGORY_BLURB[category.category] ?? '';
  const href = (`/admin/reference/${encodeURIComponent(category.category)}`) as Route;

  const updated = category.latest_updated_at
    ? new Date(category.latest_updated_at).toLocaleString()
    : 'Never synced';

  const isStale = (() => {
    if (!category.latest_updated_at) return true;
    const ageDays =
      (Date.now() - new Date(category.latest_updated_at).getTime()) /
      (1000 * 60 * 60 * 24);
    return ageDays > 7;
  })();

  return (
    <Link
      href={href}
      className="ss-card"
      style={{
        padding: '18px 20px',
        textDecoration: 'none',
        color: 'inherit',
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
      }}
    >
      <div className="ss-eyebrow">{label}</div>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          gap: 8,
        }}
      >
        <span style={{ fontSize: 26, fontWeight: 600 }}>
          {category.entry_count.toLocaleString()}
        </span>
        <span
          className="ss-badge"
          style={{
            fontSize: 11,
            color: isStale ? 'var(--danger)' : 'var(--fg-muted)',
            borderColor: isStale ? 'var(--danger)' : undefined,
          }}
        >
          {isStale ? 'Stale' : 'Fresh'}
        </span>
      </div>
      <p style={{ margin: 0, fontSize: 12, color: 'var(--fg-muted)' }}>
        {blurb}
      </p>
      <p
        style={{
          margin: '4px 0 0',
          fontSize: 12,
          color: 'var(--fg-muted)',
        }}
      >
        Last sync: <span className="mono">{updated}</span>
      </p>
    </Link>
  );
}
