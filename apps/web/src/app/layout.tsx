import type { Metadata } from 'next';
import Link from 'next/link';
import {
  getCurrentLocation,
  listSharedWithMe,
  type ResolvedLocation,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';
import { getTheme } from '@/lib/theme';
import { QuantumWarpBackground } from '@/components/shell/QuantumWarpBackground';
import { TopBar } from '@/components/shell/TopBar';
import { LeftRail } from '@/components/shell/LeftRail';
import { DrawerScrim } from '@/components/shell/DrawerScrim';
import './globals.css';

export const metadata: Metadata = {
  title: 'StarStats',
  description: 'Personal Star Citizen metrics',
};

export default async function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  const [session, theme] = await Promise.all([getSession(), getTheme()]);
  const hasSession = session !== null;

  // Per-render shell data: current location for the TopBar chip and
  // inbound-share count for the Sharing nav badge. Both fail-soft to
  // a "neutral" value (null / 0) so the shell never crashes on a
  // single API hiccup. Fetched in parallel to keep one round trip.
  let location: ResolvedLocation | null = null;
  let inboundShareCount = 0;
  if (session) {
    const [locResult, sharedResult] = await Promise.allSettled([
      getCurrentLocation(session.token),
      listSharedWithMe(session.token),
    ]);
    if (locResult.status === 'fulfilled') {
      location = locResult.value;
    } else {
      logger.warn({ err: locResult.reason }, 'topbar location fetch failed');
    }
    if (sharedResult.status === 'fulfilled') {
      inboundShareCount = sharedResult.value.shared_with_me.length;
    } else {
      logger.warn(
        { err: sharedResult.reason },
        'inbound share count fetch failed',
      );
    }
  }

  return (
    <html lang="en" data-theme={theme}>
      <body>
        <QuantumWarpBackground />
        {hasSession ? (
          <div
            className="ss-app"
            style={{ position: 'relative', zIndex: 1, minHeight: '100vh' }}
          >
            <TopBar handle={session.claimedHandle} location={location} />
            <LeftRail
              handle={session.claimedHandle}
              staffRoles={session.staffRoles}
              inboundShareCount={inboundShareCount}
            />
            <DrawerScrim />
            <div className="ss-main">
              {!session.emailVerified && (
                <div className="unverified-banner" role="status">
                  <span>Verify your Comm-Link to keep your account secure.</span>{' '}
                  <Link href="/settings#verification">Resend</Link>
                </div>
              )}
              {children}
            </div>
          </div>
        ) : (
          <div
            style={{
              position: 'relative',
              zIndex: 1,
              minHeight: '100vh',
              display: 'flex',
              flexDirection: 'column',
            }}
          >
            <div style={{ flex: 1 }}>{children}</div>
            <footer className="site-footer">
              <Link href="/privacy">Privacy</Link>
              <span aria-hidden="true">·</span>
              <a href="mailto:dojo@thecodesaiyan.io">Contact</a>
            </footer>
          </div>
        )}
      </body>
    </html>
  );
}
