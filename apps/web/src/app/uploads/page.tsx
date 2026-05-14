/**
 * Uploads — recent ingest batches the desktop client posted, read off
 * the audit log. Per the project's "no raw retention" decision this is
 * metadata-only: there's no per-line drill-down or batch retry.
 *
 * Powered by GET /v1/me/ingest-history. Pagination is offset-based via
 * the `?offset=` search param; the page also exposes a small summary
 * strip (last 24h totals) derived from the same response.
 *
 * Sibling surface: /metrics?view=raw is the per-event stream — this
 * page is the per-batch upload audit.
 */

import Link from 'next/link';
import type { Route } from 'next';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  getIngestHistory,
  type IngestBatchDto,
  type IngestHistoryResponse,
} from '@/lib/api';
import { getSession } from '@/lib/session';

const PAGE_LIMIT = 50;

interface SearchParams {
  offset?: string;
}

export default async function UploadsPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/uploads');

  const params = await props.searchParams;
  const offset = parseOffset(params.offset);

  let history: IngestHistoryResponse;
  try {
    history = await getIngestHistory(session.token, {
      limit: PAGE_LIMIT,
      offset,
    });
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/uploads');
    }
    throw e;
  }

  const summary = computeWindowSummary(history.batches);
  const hasOlder = history.batches.length === PAGE_LIMIT;
  const hasNewer = offset > 0;
  const olderHref = `/uploads?offset=${offset + PAGE_LIMIT}` as Route;
  const newerHref =
    offset - PAGE_LIMIT <= 0
      ? ('/uploads' as Route)
      : (`/uploads?offset=${offset - PAGE_LIMIT}` as Route);

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Uploads · what your client has shipped
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          Ingest history
        </h1>
        <p style={{ margin: '6px 0 0', color: 'var(--fg-muted)', fontSize: 14 }}>
          Every batch your desktop client has posted, with the server&apos;s
          accept / duplicate / reject verdict. Raw lines are not retained —
          only the per-batch counts you see here.
        </p>
      </header>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(4, minmax(0, 1fr))',
          gap: 12,
        }}
      >
        <StatTile
          label="Batches in view"
          value={history.batches.length.toLocaleString()}
        />
        <StatTile label="Events posted" value={summary.total.toLocaleString()} />
        <StatTile
          label="Accepted"
          value={summary.accepted.toLocaleString()}
          tone="ok"
        />
        <StatTile
          label="Rejected"
          value={summary.rejected.toLocaleString()}
          tone={summary.rejected > 0 ? 'danger' : 'dim'}
        />
      </div>

      <section className="ss-card">
        <div className="ss-eyebrow" style={{ marginBottom: 12 }}>
          {history.batches.length > 0
            ? `Recent batches · showing ${history.batches.length}`
            : 'Recent batches'}
        </div>
        {history.batches.length === 0 ? (
          <p
            style={{
              margin: '8px 0',
              color: 'var(--fg-dim)',
              fontSize: 13,
            }}
          >
            Scope is clear. Once your desktop client posts a batch it will
            appear here.
          </p>
        ) : (
          <div className="ss-table-wrap">
            <table className="ss-table" style={{ fontSize: 13 }}>
              <thead>
                <tr>
                  <th style={{ textAlign: 'left' }}>When</th>
                  <th style={{ textAlign: 'left' }}>Batch</th>
                  <th style={{ textAlign: 'left' }}>Build</th>
                  <th style={{ textAlign: 'right' }}>Total</th>
                  <th style={{ textAlign: 'right' }}>Accepted</th>
                  <th style={{ textAlign: 'right' }}>Duplicate</th>
                  <th style={{ textAlign: 'right' }}>Rejected</th>
                </tr>
              </thead>
              <tbody>
                {history.batches.map((b) => (
                  <BatchRow key={b.seq} batch={b} />
                ))}
              </tbody>
            </table>
          </div>
        )}

        {(hasOlder || hasNewer) && (
          <div
            style={{
              display: 'flex',
              gap: 12,
              marginTop: 14,
              paddingTop: 14,
              borderTop: '1px solid var(--border)',
            }}
          >
            {hasNewer && (
              <Link href={newerHref} className="ss-btn ss-btn--ghost">
                ← Newer
              </Link>
            )}
            {hasOlder && (
              <Link
                href={olderHref}
                className="ss-btn ss-btn--ghost"
                style={{ marginLeft: 'auto' }}
              >
                Older →
              </Link>
            )}
          </div>
        )}
      </section>
    </div>
  );
}

function BatchRow({ batch }: { batch: IngestBatchDto }) {
  const rejectionPct =
    batch.total > 0 ? (batch.rejected / batch.total) * 100 : 0;
  return (
    <tr>
      <td style={{ color: 'var(--fg-muted)' }}>{formatRelativeTime(batch.occurred_at)}</td>
      <td>
        <span
          className="mono"
          style={{ fontSize: 12, color: 'var(--fg-dim)' }}
          title={batch.batch_id}
        >
          {shortenBatchId(batch.batch_id)}
        </span>
      </td>
      <td className="mono" style={{ fontSize: 12, color: 'var(--fg-muted)' }}>
        {batch.game_build ?? '—'}
      </td>
      <td className="mono" style={{ textAlign: 'right' }}>
        {batch.total.toLocaleString()}
      </td>
      <td
        className="mono"
        style={{ textAlign: 'right', color: 'var(--ok)' }}
      >
        {batch.accepted.toLocaleString()}
      </td>
      <td
        className="mono"
        style={{ textAlign: 'right', color: 'var(--fg-dim)' }}
      >
        {batch.duplicate.toLocaleString()}
      </td>
      <td
        className="mono"
        style={{
          textAlign: 'right',
          color:
            batch.rejected > 0
              ? rejectionPct > 5
                ? 'var(--danger)'
                : 'var(--warn)'
              : 'var(--fg-dim)',
        }}
      >
        {batch.rejected.toLocaleString()}
      </td>
    </tr>
  );
}

interface StatTileProps {
  label: string;
  value: string;
  tone?: 'ok' | 'danger' | 'dim';
}

function StatTile({ label, value, tone }: StatTileProps) {
  const colour =
    tone === 'ok'
      ? 'var(--ok)'
      : tone === 'danger'
        ? 'var(--danger)'
        : tone === 'dim'
          ? 'var(--fg-dim)'
          : 'var(--fg)';
  return (
    <div className="ss-card" style={{ padding: '16px 18px' }}>
      <div className="ss-eyebrow">{label}</div>
      <div
        className="mono"
        style={{
          fontSize: 24,
          fontWeight: 500,
          letterSpacing: '-0.015em',
          margin: '6px 0 0',
          color: colour,
        }}
      >
        {value}
      </div>
    </div>
  );
}

function parseOffset(raw: string | undefined): number {
  if (!raw) return 0;
  const n = Number(raw);
  if (!Number.isFinite(n) || n < 0) return 0;
  return Math.floor(n);
}

interface WindowSummary {
  total: number;
  accepted: number;
  rejected: number;
}

function computeWindowSummary(batches: IngestBatchDto[]): WindowSummary {
  return batches.reduce<WindowSummary>(
    (acc, b) => ({
      total: acc.total + b.total,
      accepted: acc.accepted + b.accepted,
      rejected: acc.rejected + b.rejected,
    }),
    { total: 0, accepted: 0, rejected: 0 },
  );
}

function shortenBatchId(id: string): string {
  if (id.length <= 12) return id;
  return `${id.slice(0, 8)}…${id.slice(-3)}`;
}

function formatRelativeTime(iso: string): string {
  const ts = new Date(iso).getTime();
  if (Number.isNaN(ts)) return iso;
  const diffMs = Date.now() - ts;
  if (diffMs < 60_000) return 'just now';
  if (diffMs < 3_600_000) return `${Math.floor(diffMs / 60_000)}m ago`;
  if (diffMs < 86_400_000) return `${Math.floor(diffMs / 3_600_000)}h ago`;
  if (diffMs < 7 * 86_400_000) return `${Math.floor(diffMs / 86_400_000)}d ago`;
  return new Date(iso).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  });
}
