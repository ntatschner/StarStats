import type { Metadata, Route } from 'next';
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
      // Count active shares only — expired entries still appear in
      // the inbound list (recipients should know who used to share)
      // but the nav badge should reflect "things to look at now". An
      // expired badge would be noise and would never clear.
      const now = Date.now();
      inboundShareCount = sharedResult.value.shared_with_me.filter(
        (entry) =>
          !entry.expires_at ||
          new Date(entry.expires_at).getTime() > now,
      ).length;
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
                  {/*
                    Voice polish from the design audit v2 (§08): the
                    previous "Verify your Comm-Link to keep your
                    account secure" was dry copy that broke from the
                    in-universe register the rest of the product uses.
                  */}
                  <span>Comm-Link unverified — claim it before someone else can.</span>{' '}
                  <Link href="/settings#verification">Resend</Link>
                </div>
              )}
              {children}
            </div>
            {/*
              Audit v2 §07 polish: Lore moved out of the left rail into
              a calm footer — users hit it once after signup, so the
              rail entry was noise. Fine-print + privacy live here too.
            */}
            <footer
              style={{
                textAlign: 'center',
                fontSize: 'var(--fs-xs)',
                color: 'var(--fg-dim)',
                padding: 'var(--s4) var(--s5)',
              }}
            >
              <Link href={'/lore' as Route} style={{ color: 'inherit' }}>
                Lore
              </Link>
              <span aria-hidden="true"> · </span>
              <Link href="/privacy" style={{ color: 'inherit' }}>
                Privacy
              </Link>
              <span aria-hidden="true"> · </span>
              <a
                href="mailto:dojo@thecodesaiyan.io"
                style={{ color: 'inherit' }}
              >
                Contact
              </a>
            </footer>
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
