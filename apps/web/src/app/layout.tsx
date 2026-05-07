import type { Metadata } from 'next';
import Link from 'next/link';
import { getSession } from '@/lib/session';
import { getTheme } from '@/lib/theme';
import { QuantumWarp } from '@/components/shell/QuantumWarp';
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

  return (
    <html lang="en" data-theme={theme}>
      <body>
        <QuantumWarp />
        {hasSession ? (
          <div
            className="ss-app"
            style={{ position: 'relative', zIndex: 1, minHeight: '100vh' }}
          >
            <TopBar handle={session.claimedHandle} />
            <LeftRail handle={session.claimedHandle} />
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
