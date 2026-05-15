import Link from 'next/link';
import { redirect } from 'next/navigation';
import { getSession } from '@/lib/session';
import { HeroRotator } from './_components/HeroRotator';
import { HeroHeatmap } from './_components/HeroHeatmap';

const FEATURES: ReadonlyArray<{
  title: string;
  body: string;
  glyph: string;
}> = [
  {
    glyph: '◇',
    title: 'Stays on your PC by default',
    body:
      "Nothing leaves your machine until you sign in and turn on sync. You're in charge of when it talks to us.",
  },
  {
    glyph: '▦',
    title: 'Just what you did in-game',
    body:
      "Logins, deaths, missions, jumps. Never your chat, never other players, never anything the game doesn't already show you.",
  },
  {
    glyph: '↗',
    title: 'Share what you want, exactly',
    body:
      'Per-event visibility on top of profile-level controls: public, RSI org-only, named-handle grants with expiry, or fully private. Verify a handle is yours by pasting a code into your bio for a minute.',
  },
  {
    glyph: '◈',
    title: 'Org workspaces',
    body:
      'RSI org owners get a shared dashboard with roles enforced by Zanzibar-style ReBAC — per-org privacy is a relationship check, not a row filter.',
  },
  {
    glyph: '✦',
    title: 'Tracks every PC you play on',
    body:
      'Add as many devices as you like. Everything lands in one timeline, no double-counts, no gaps.',
  },
  {
    glyph: '⤓',
    title: 'Your numbers, your file',
    body:
      'Per-day heatmap, top activities, full timeline. Download the whole manifest as a single file whenever you want.',
  },
  {
    glyph: '⊞',
    title: 'Locked-down sign-in',
    body:
      'Magic link or password, two-factor with backup codes, per-device pairing. Handle verification stops anyone claiming yours.',
  },
  {
    glyph: '⌘',
    title: 'Self-host the whole stack',
    body:
      'Bring your own Postgres + SpiceDB and run the API on your own box. Comes with a built-in admin console for roles, orgs, audit log, and reference-data inspection.',
  },
];

export default async function HomePage() {
  const session = await getSession();
  if (session) redirect('/dashboard');

  return (
    <div className="ss-landing" style={{ minHeight: '100%', position: 'relative' }}>
      {/* Marketing top bar (lighter than the app shell). */}
      <header
        className="ss-marketing-nav"
        style={{
          display: 'flex',
          alignItems: 'center',
          padding: '20px 48px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--bg)',
          position: 'relative',
          zIndex: 1,
        }}
      >
        <span className="ss-mark">
          <span className="ss-mark-glyph">★</span>
          <span>STARSTATS</span>
        </span>
        <span
          className="ss-eyebrow"
          style={{ marginLeft: 12, color: 'var(--fg-dim)', fontWeight: 500 }}
        >
          Community telemetry · Unofficial
        </span>
        <div style={{ flex: 1 }} />
        <nav
          style={{
            display: 'flex',
            gap: 28,
            alignItems: 'center',
            color: 'var(--fg-muted)',
            fontSize: 13,
          }}
        >
          <a href="#features" style={{ color: 'inherit', textDecoration: 'none' }}>
            Features
          </a>
          <Link href="/privacy" style={{ color: 'inherit', textDecoration: 'none' }}>
            Privacy
          </Link>
          <a
            href="https://github.com/ntatschner/StarStats/releases"
            target="_blank"
            rel="noreferrer noopener"
            style={{ color: 'inherit', textDecoration: 'none' }}
          >
            Download
          </a>
          <a
            href="https://github.com/ntatschner/StarStats"
            target="_blank"
            rel="noreferrer noopener"
            style={{ color: 'inherit', textDecoration: 'none' }}
            title="View source on GitHub"
          >
            GitHub
          </a>
          <Link href="/auth/login" className="ss-btn ss-btn--ghost">
            Sign in
          </Link>
          <Link href="/auth/signup" className="ss-btn ss-btn--primary">
            Get started →
          </Link>
        </nav>
      </header>

      <main
        style={{
          position: 'relative',
          zIndex: 1,
          maxWidth: 'none',
          margin: 0,
          padding: 0,
        }}
      >
        {/* Hero */}
        <section
          className="ss-hero"
          style={{
            padding: '120px 48px 80px',
            maxWidth: 1280,
            margin: '0 auto',
            display: 'grid',
            gridTemplateColumns: '1.2fr 1fr',
            gap: 64,
          }}
        >
          <div>
            <div
              className="ss-eyebrow"
              style={{ marginBottom: 24, color: 'var(--accent)' }}
            >
              <span
                style={{
                  display: 'inline-block',
                  width: 6,
                  height: 6,
                  borderRadius: 999,
                  background: 'var(--accent)',
                  marginRight: 8,
                  verticalAlign: 'middle',
                }}
              />
              v0.0.3-beta · Pyro patch ready
            </div>
            <h1
              style={{
                margin: 0,
                fontWeight: 600,
                fontSize: 'clamp(40px, 6vw, 76px)',
                lineHeight: 1.02,
                letterSpacing: '-0.025em',
                color: 'var(--fg)',
              }}
            >
              <span
                style={{
                  color: 'var(--fg-muted)',
                  fontSize: '0.6em',
                  display: 'block',
                  marginBottom: 12,
                  fontWeight: 400,
                  letterSpacing: '-0.01em',
                }}
              >
                Track your Star Citizen play.
              </span>
              <HeroRotator />
            </h1>
            <p
              style={{
                color: 'var(--fg-muted)',
                fontSize: 18,
                lineHeight: 1.55,
                maxWidth: 560,
                marginTop: 28,
              }}
            >
              A small app on your PC reads what the game already writes
              down — when you log in, where you fly, what you fly with. It
              stays on your machine until you sign in and turn on sync.
              Never your chat. Never anyone else.
            </p>
            <div
              className="ss-hero-buttons"
              style={{ display: 'flex', gap: 12, marginTop: 36 }}
            >
              <Link href="/auth/signup" className="ss-btn ss-btn--primary">
                Create account →
              </Link>
              <a
                href="https://github.com/ntatschner/StarStats/releases"
                target="_blank"
                rel="noreferrer noopener"
                className="ss-btn ss-btn--ghost"
              >
                Download tray client
              </a>
            </div>
            <div
              style={{
                marginTop: 32,
                display: 'flex',
                gap: 24,
                alignItems: 'center',
                color: 'var(--fg-dim)',
                fontSize: 12,
              }}
            >
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                ✓ Local-first
              </span>
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                ✓ Open source client
              </span>
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                ✓ Not affiliated with CIG
              </span>
            </div>
          </div>

          {/* Hero mockup — stylised stat card preview */}
          <div
            className="ss-hero-mock"
            style={{
              position: 'relative',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <div
              style={{
                position: 'absolute',
                inset: -40,
                background:
                  'radial-gradient(circle at 60% 40%, var(--accent-glow), transparent 60%)',
                filter: 'blur(20px)',
                opacity: 0.5,
                pointerEvents: 'none',
              }}
            />
            <div
              className="ss-card ss-card--elev"
              style={{
                position: 'relative',
                width: '100%',
                maxWidth: 460,
                padding: '22px 24px',
                transform: 'rotate(-1deg)',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  marginBottom: 18,
                }}
              >
                <div className="ss-eyebrow">Last 26 weeks</div>
                <span className="ss-badge ss-badge--accent">
                  <span className="ss-badge-dot" />
                  Live
                </span>
              </div>
              <HeroHeatmap />
              <hr className="ss-rule" style={{ margin: '18px 0' }} />
              <div
                style={{
                  display: 'flex',
                  gap: 20,
                  alignItems: 'center',
                  flexWrap: 'nowrap',
                }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <div className="ss-eyebrow">Sessions</div>
                  <div
                    className="mono"
                    style={{
                      fontSize: 22,
                      color: 'var(--fg)',
                      letterSpacing: '-0.01em',
                    }}
                  >
                    184
                  </div>
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <div className="ss-eyebrow">Hours</div>
                  <div
                    className="mono"
                    style={{
                      fontSize: 22,
                      color: 'var(--fg)',
                      letterSpacing: '-0.01em',
                    }}
                  >
                    312.7
                  </div>
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <div className="ss-eyebrow">Top type</div>
                  <div
                    className="mono"
                    style={{ fontSize: 13, color: 'var(--accent)' }}
                  >
                    quantum_target
                  </div>
                </div>
              </div>
            </div>
          </div>
        </section>

        {/* Features grid */}
        <section
          id="features"
          className="ss-features"
          style={{
            maxWidth: 1280,
            margin: '0 auto',
            padding: '0 48px 100px',
          }}
        >
          <div className="ss-eyebrow" style={{ marginBottom: 16 }}>
            What you get
          </div>
          <h2
            style={{
              margin: '0 0 48px',
              fontSize: 32,
              fontWeight: 600,
              letterSpacing: '-0.02em',
              color: 'var(--fg)',
              maxWidth: 720,
            }}
          >
            A telemetry tool, not a fan shrine. Built for players who want
            to read their own footprint.
          </h2>

          <div
            data-rspgrid="3"
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(3, 1fr)',
              gap: 16,
            }}
          >
            {FEATURES.map((f) => (
              <div key={f.title} className="ss-card" style={{ padding: 22 }}>
                <div
                  style={{
                    width: 32,
                    height: 32,
                    borderRadius: 8,
                    background: 'var(--accent-soft)',
                    color: 'var(--accent)',
                    display: 'grid',
                    placeItems: 'center',
                    marginBottom: 14,
                    border:
                      '1px solid color-mix(in oklab, var(--accent) 30%, transparent)',
                    fontSize: 16,
                    fontFamily: 'var(--font-mono)',
                  }}
                  aria-hidden
                >
                  {f.glyph}
                </div>
                <h3
                  style={{
                    margin: '0 0 8px',
                    fontSize: 15,
                    fontWeight: 600,
                    letterSpacing: '-0.01em',
                  }}
                >
                  {f.title}
                </h3>
                <p
                  style={{
                    margin: 0,
                    color: 'var(--fg-muted)',
                    fontSize: 13,
                    lineHeight: 1.55,
                  }}
                >
                  {f.body}
                </p>
              </div>
            ))}
          </div>
        </section>
      </main>
    </div>
  );
}
