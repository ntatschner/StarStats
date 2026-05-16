/**
 * /metrics — deprecated. Per design audit v2 §07 the manifest viewer
 * was merged into `/journey?view=types`. This stub preserves any
 * incoming query string (the legacy `?type=…` filter, login `next=`
 * targets, external bookmarks) and forwards to the new home.
 */
import { redirect } from 'next/navigation';
import type { Route } from 'next';

export default async function MetricsPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const sp = await searchParams;
  const qs = new URLSearchParams();
  qs.set('view', 'types');
  // Forward known scalar params verbatim. Multi-value params are
  // skipped — the legacy page never produced them.
  for (const [k, v] of Object.entries(sp)) {
    if (k === 'view') continue;
    if (typeof v === 'string') qs.set(k, v);
  }
  redirect(`/journey?${qs.toString()}` as Route);
}
