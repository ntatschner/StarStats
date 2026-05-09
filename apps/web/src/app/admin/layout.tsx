import { redirect } from 'next/navigation';
import { getSession } from '@/lib/session';

/**
 * Server-component gate for the /admin surface.
 *
 * Runs before any /admin/** page renders. Uses the `staffRoles` field
 * mirrored into the session cookie at sign-in time, so role checks
 * don't pay an extra `/v1/auth/me` round trip per nav.
 *
 * Note: this is UX gating only. The API endpoints under
 * `/v1/admin/...` enforce the same check server-side via
 * `StaffRoleSet::has`, so a tampered cookie can't escalate.
 *
 * Admin implies moderator on the server side, so we accept either.
 */
export default async function AdminLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const session = await getSession();
  if (!session) {
    redirect('/auth/login?next=/admin');
  }
  const isStaff = session.staffRoles.some(
    (r) => r === 'admin' || r === 'moderator',
  );
  if (!isStaff) {
    redirect('/dashboard');
  }
  return <>{children}</>;
}
