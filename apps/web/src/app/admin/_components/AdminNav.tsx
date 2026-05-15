/**
 * Admin section sub-navigation. Imported by individual admin pages
 * rather than the layout so the layout stays minimal (D1 owns it
 * and only enforces auth/role gating).
 *
 * `current` controls which link renders as active.
 */

import Link from 'next/link';
import type { Route } from 'next';

type CurrentTab =
  | 'dashboard'
  | 'submissions'
  | 'users'
  | 'orgs'
  | 'reference'
  | 'smtp'
  | 'audit';

const TABS: ReadonlyArray<{
  id: CurrentTab;
  label: string;
  /** When undefined, the link is disabled (placeholder for future slice). */
  href: Route | undefined;
  /** Free-form tag rendered after the label, e.g. "Slice 5". */
  tag?: string;
}> = [
  { id: 'dashboard', label: 'Dashboard', href: '/admin' as Route },
  {
    id: 'submissions',
    label: 'Submissions',
    // Default the queue filter to `review` so the link is canonical
    // ("nothing to triage" lands you on the right bucket).
    href: '/admin/submissions?status=review' as Route,
  },
  { id: 'users', label: 'Users', href: '/admin/users' as Route },
  { id: 'orgs', label: 'Orgs', href: '/admin/orgs' as Route },
  { id: 'reference', label: 'Reference', href: '/admin/reference' as Route },
  { id: 'smtp', label: 'SMTP', href: '/admin/smtp' as Route },
  {
    // Audit log viewer landed alongside the admin build-out — the
    // tab is live now.
    id: 'audit',
    label: 'Audit log',
    href: '/admin/audit' as Route,
  },
];

export function AdminNav({ current }: { current: CurrentTab }) {
  return (
    <nav
      aria-label="Admin sections"
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 6,
        borderBottom: '1px solid var(--border)',
        paddingBottom: 12,
        marginBottom: 4,
      }}
    >
      {TABS.map((t) => {
        const active = t.id === current;
        const disabled = t.href === undefined;

        const baseStyle: React.CSSProperties = {
          padding: '8px 14px',
          borderRadius: 'var(--r-pill)',
          fontSize: 13,
          textDecoration: 'none',
          display: 'inline-flex',
          alignItems: 'center',
          gap: 8,
          border: '1px solid',
        };

        if (disabled) {
          return (
            <span
              key={t.id}
              aria-disabled="true"
              style={{
                ...baseStyle,
                background: 'transparent',
                borderColor: 'transparent',
                color: 'var(--fg-dim)',
                cursor: 'not-allowed',
              }}
            >
              <span>{t.label}</span>
              {t.tag && (
                <span
                  className="ss-badge"
                  style={{ fontSize: 10 }}
                >
                  {t.tag}
                </span>
              )}
            </span>
          );
        }

        return (
          <Link
            key={t.id}
            href={t.href!}
            data-active={active ? 'true' : undefined}
            style={{
              ...baseStyle,
              background: active ? 'var(--bg-elev)' : 'transparent',
              borderColor: active ? 'var(--border-strong)' : 'transparent',
              color: active ? 'var(--fg)' : 'var(--fg-muted)',
            }}
          >
            <span>{t.label}</span>
          </Link>
        );
      })}
    </nav>
  );
}
