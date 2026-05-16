import Link from 'next/link';
import { revalidatePath } from 'next/cache';
import { redirect } from 'next/navigation';
import {
  ApiCallError,
  changePassword,
  deleteAccount,
  emailChangeStart,
  getMe,
  getMyHangar,
  getMyProfile,
  getPreferences,
  refreshProfile,
  refreshRsiOrgs,
  resendVerification,
  rsiVerifyCheck,
  rsiVerifyStart,
  type HangarSnapshot,
  type MeResponse,
  type ProfileResponse,
  type RsiStartResponse,
} from '@/lib/api';
import { HangarCard } from '@/components/HangarCard';
import { logger } from '@/lib/logger';
import { clearSession, getSession } from '@/lib/session';
import { getTheme, isTheme, setTheme, type Theme } from '@/lib/theme';
import { SecuritySection } from './_components/SecuritySection';

interface SearchParams {
  status?: string;
  error?: string;
}

// ----- Layout style helpers ------------------------------------------------
//
// We're inside `.ss-main` (provided by the app shell in layout.tsx), so the
// page itself just needs a centered stack of cards. No `.dashboard` wrapper.

const pageStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 20,
  maxWidth: 960,
  margin: '0 auto',
  padding: '8px 0 60px',
};

const headerEyebrowStyle: React.CSSProperties = {
  marginBottom: 8,
};

const titleStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 32,
  fontWeight: 600,
  letterSpacing: '-0.02em',
};

const subtitleStyle: React.CSSProperties = {
  margin: '6px 0 0',
  color: 'var(--fg-muted)',
  fontSize: 14,
  lineHeight: 1.55,
};

const cardHeaderStyle: React.CSSProperties = {
  padding: '20px 24px 0',
};

const cardBodyStyle: React.CSSProperties = {
  padding: '16px 24px 22px',
  display: 'flex',
  flexDirection: 'column',
  gap: 14,
};

const cardFooterStyle: React.CSSProperties = {
  padding: '14px 24px',
};

const cardTitleStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 17,
  fontWeight: 600,
  letterSpacing: '-0.01em',
  color: 'var(--fg)',
};

const dangerCardStyle: React.CSSProperties = {
  borderColor: 'color-mix(in oklab, var(--danger) 35%, transparent)',
};

const dangerCardTitleStyle: React.CSSProperties = {
  ...cardTitleStyle,
  color: 'var(--danger)',
};

const formStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 12,
  margin: 0,
};

const formRowEndStyle: React.CSSProperties = {
  display: 'flex',
  gap: 8,
  flexWrap: 'wrap',
  alignItems: 'flex-end',
};

const mutedStyle: React.CSSProperties = {
  margin: 0,
  color: 'var(--fg-muted)',
  fontSize: 13,
  lineHeight: 1.55,
};

const dimStyle: React.CSSProperties = {
  color: 'var(--fg-dim)',
  fontSize: 12,
};

const themeGridStyle: React.CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(4, minmax(0, 1fr))',
  gap: 12,
};

const monoStyle: React.CSSProperties = {
  fontFamily: 'var(--font-mono)',
};

// ----- Theme switcher (Wave 8.3 — wired to ss-theme cookie + backend) ------

interface ThemePreview {
  id: Theme;
  name: string;
  subtitle: string;
  /** Five accent colors used to paint the static preview chrome. */
  swatch: [string, string, string, string, string];
}

const THEME_PREVIEWS: readonly ThemePreview[] = [
  {
    id: 'stanton',
    name: 'Stanton',
    subtitle: 'Default · warm amber',
    swatch: ['#15131A', '#1A1820', '#2A2734', '#E8A23C', '#ECE7DD'],
  },
  {
    id: 'pyro',
    name: 'Pyro',
    subtitle: 'Molten coral · aggressive',
    swatch: ['#1A1213', '#1F1517', '#321F22', '#F25C3F', '#F2E6E0'],
  },
  {
    id: 'terra',
    name: 'Terra',
    subtitle: 'Cool teal · clinical',
    swatch: ['#0F161B', '#131C22', '#1F2C36', '#4FB8A1', '#E2EAEC'],
  },
  {
    id: 'nyx',
    name: 'Nyx',
    subtitle: 'Light · deep violet',
    swatch: ['#ECE8E1', '#F7F4EE', '#FFFFFF', '#5B3FD9', '#1B1722'],
  },
] as const;

/**
 * Submit button styled as a swatch card. Sits inside the parent
 * `<form action={themeAction}>` and submits its `theme` value when
 * clicked, so the server action picks up the requested theme via
 * `formData.get('theme')`.
 */
function ThemeSwatchButton({
  theme,
  active,
}: {
  theme: ThemePreview;
  active: boolean;
}) {
  return (
    <button
      type="submit"
      name="theme"
      value={theme.id}
      className="ss-theme-swatch"
      data-active={active ? 'true' : undefined}
      aria-pressed={active}
      aria-label={`Switch to ${theme.name} theme`}
      style={{
        background: theme.swatch[1],
        color: theme.swatch[4],
        cursor: 'pointer',
        font: 'inherit',
        textAlign: 'left',
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'flex-start',
          width: '100%',
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <span
            style={{
              fontWeight: 600,
              fontSize: 14,
              letterSpacing: '-0.01em',
            }}
          >
            {theme.name}
          </span>
          <span style={{ fontSize: 11, opacity: 0.7 }}>{theme.subtitle}</span>
        </div>
        {active && (
          <span
            style={{
              width: 22,
              height: 22,
              borderRadius: 999,
              background: theme.swatch[3],
              color: theme.swatch[0],
              display: 'grid',
              placeItems: 'center',
              flexShrink: 0,
              fontSize: 12,
              fontWeight: 700,
            }}
            aria-hidden="true"
          >
            ✓
          </span>
        )}
      </div>
      <div style={{ flex: 1 }} />
      <div
        style={{
          background: theme.swatch[2],
          borderRadius: 5,
          padding: '6px 8px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 6,
        }}
      >
        <span
          style={{
            fontSize: 10,
            fontFamily: 'var(--font-mono)',
            opacity: 0.7,
          }}
        >
          ★ STARSTATS
        </span>
        <span
          style={{
            height: 6,
            background: theme.swatch[3],
            borderRadius: 3,
            width: 36,
          }}
        />
      </div>
      <div className="ss-theme-swatch-bars">
        {[0.2, 0.4, 0.7, 1].map((o, i) => (
          <span
            key={i}
            style={{ background: theme.swatch[3], opacity: o }}
          />
        ))}
      </div>
    </button>
  );
}

// ---------------------------------------------------------------------------

export default async function SettingsPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/settings');

  const { status, error } = await props.searchParams;

  // /v1/auth/me is the source of truth — the cookie may be stale.
  let me: MeResponse;
  try {
    me = await getMe(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/settings');
    }
    throw e;
  }

  // Sharing state moved to /sharing in 0.0.4-beta — no longer
  // loaded here. See apps/web/src/app/sharing/page.tsx.

  // Pull (or issue) the RSI verification code only when the handle
  // isn't proven yet — already-verified users don't need a code.
  // Failure here degrades to "couldn't load" rather than throwing,
  // because verification is opt-in and the rest of the page is
  // unrelated.
  let rsiState: RsiStartResponse | null = null;
  let rsiLoadFailed = false;
  if (!me.rsi_verified) {
    try {
      rsiState = await rsiVerifyStart(session.token);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        redirect('/auth/login?next=/settings');
      }
      logger.warn({ err: e }, 'rsi verify start failed');
      rsiLoadFailed = true;
    }
  }

  // Active theme. Source-of-truth order:
  //   1. server-side preferences row (follows user across devices)
  //   2. local `ss-theme` cookie (last-write-wins for this browser)
  //   3. DEFAULT_THEME (Stanton)
  // The PUT /v1/me/preferences endpoint is fresh from Wave 8.3 backend —
  // if it errors, fall through to the cookie so the page still paints.
  let activeTheme: Theme = await getTheme();
  try {
    const prefs = await getPreferences(session.token);
    if (isTheme(prefs.theme)) activeTheme = prefs.theme;
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/settings');
    }
    logger.warn({ err: e }, 'load preferences failed');
    // fall through — cookie value (or DEFAULT_THEME) already in activeTheme
  }

  // Profile snapshot — only meaningful for verified users (the
  // refresh endpoint 422s otherwise, and the snapshot cache is keyed
  // off the verified handle). 404 = "no snapshot yet"; any other
  // error degrades to "load failed" so the rest of the page still
  // renders.
  let profile: ProfileResponse | null = null;
  let profileLoadFailed = false;
  if (me.rsi_verified) {
    try {
      profile = await getMyProfile(session.token);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        redirect('/auth/login?next=/settings');
      }
      if (!(e instanceof ApiCallError) || e.status !== 404) {
        logger.warn({ err: e }, 'load my profile snapshot failed');
        profileLoadFailed = true;
      }
    }
  }

  // Hangar snapshot — pushed by the tray client, not the website.
  // Surfaced here so users can confirm the tray is talking to the
  // server without launching the tray itself. `getMyHangar` already
  // converts 404 ("no_hangar_yet") into a typed null, so the only
  // surprise we have to catch is a 401 (session expired).
  // Hangar sync is independent of RSI verification — pairing a
  // device is sufficient.
  let hangar: HangarSnapshot | null = null;
  try {
    hangar = await getMyHangar(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/settings');
    }
    logger.warn({ err: e }, 'load hangar snapshot failed');
  }

  async function themeAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings#theme');
    const raw = formData.get('theme');
    if (!isTheme(raw)) {
      // Form was tampered with or submitted without a button value —
      // ignore silently rather than error out, themes aren't load-bearing.
      redirect('/settings?error=invalid_theme#theme');
    }
    // setTheme writes the cookie and forwards to PUT /v1/me/preferences;
    // backend failures are logged + swallowed so the cookie still wins
    // for this browser.
    await setTheme(raw, session.token);
    revalidatePath('/settings');
    redirect('/settings?status=theme_updated#theme');
  }

  async function resendAction() {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings');
    try {
      await resendVerification(session.token);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        redirect('/auth/login?next=/settings');
      }
      if (e instanceof ApiCallError && e.status === 409) {
        redirect('/settings?status=already_verified#verification');
      }
      logger.error({ err: e }, 'resend verification failed');
      redirect('/settings?error=unexpected#verification');
    }
    redirect('/settings?status=resent#verification');
  }

  async function rsiCheckAction() {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings');
    let verified = false;
    try {
      const resp = await rsiVerifyCheck(session.token);
      verified = resp.verified;
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/settings');
        if (e.status === 422) redirect('/settings?error=rsi_code_not_in_bio#rsi');
        if (e.status === 404) redirect('/settings?error=rsi_handle_not_found#rsi');
        if (e.status === 410) redirect('/settings?error=rsi_code_expired#rsi');
        if (e.status === 503) redirect('/settings?error=rsi_unavailable#rsi');
      }
      logger.error({ err: e }, 'rsi verify check failed');
      redirect('/settings?error=unexpected#rsi');
    }
    redirect(
      verified
        ? '/settings?status=rsi_verified#rsi'
        : '/settings?error=rsi_unknown#rsi',
    );
  }

  async function refreshProfileAction() {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings');
    try {
      await refreshProfile(session.token);
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/settings');
        if (e.status === 422) {
          redirect('/settings?error=rsi_handle_not_verified#rsi');
        }
        if (e.status === 429) redirect('/settings?error=refresh_too_soon#rsi');
        if (e.status === 404) {
          redirect('/settings?error=rsi_handle_not_found#rsi');
        }
        if (e.status === 503) redirect('/settings?error=rsi_unavailable#rsi');
      }
      logger.error({ err: e }, 'refresh profile failed');
      redirect('/settings?error=unexpected#rsi');
    }
    redirect('/settings?status=profile_refreshed#rsi');
  }

  async function refreshRsiOrgsAction() {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings');
    try {
      await refreshRsiOrgs(session.token);
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/settings');
        if (e.status === 422) {
          redirect('/settings?error=rsi_handle_not_verified#rsi');
        }
        if (e.status === 429) {
          redirect('/settings?error=orgs_refresh_too_soon#rsi');
        }
        if (e.status === 404) {
          redirect('/settings?error=rsi_handle_not_found#rsi');
        }
        if (e.status === 503) redirect('/settings?error=rsi_unavailable#rsi');
      }
      logger.error({ err: e }, 'refresh rsi orgs failed');
      redirect('/settings?error=unexpected#rsi');
    }
    redirect('/settings?status=orgs_refreshed#rsi');
  }

  async function emailChangeAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings');
    const new_email = String(formData.get('new_email') ?? '').trim();
    if (new_email === '') {
      redirect('/settings?error=invalid_email#email');
    }
    try {
      await emailChangeStart(session.token, { new_email });
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/settings');
        if (e.status === 409) redirect('/settings?error=email_taken#email');
        if (e.status === 400) {
          redirect(
            `/settings?error=${encodeURIComponent(e.body.error)}#email`,
          );
        }
      }
      logger.error({ err: e }, 'email change start failed');
      redirect('/settings?error=unexpected#email');
    }
    redirect('/settings?status=email_change_sent#email');
  }

  async function passwordAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings');

    const current_password = String(formData.get('current_password') ?? '');
    const new_password = String(formData.get('new_password') ?? '');

    try {
      await changePassword(session.token, { current_password, new_password });
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) {
          redirect('/settings?error=invalid_credentials#password');
        }
        if (e.status === 400) {
          redirect(
            `/settings?error=${encodeURIComponent(e.body.error)}#password`,
          );
        }
      }
      logger.error({ err: e }, 'change password failed');
      redirect('/settings?error=unexpected#password');
    }
    redirect('/settings?status=password_changed#password');
  }

  async function deleteAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings');

    const confirm_handle = String(formData.get('confirm_handle') ?? '').trim();

    try {
      await deleteAccount(session.token, { confirm_handle });
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) {
          redirect('/auth/login?next=/settings');
        }
        if (e.status === 400) {
          redirect(
            `/settings?error=${encodeURIComponent(e.body.error)}#danger`,
          );
        }
      }
      logger.error({ err: e }, 'delete account failed');
      redirect('/settings?error=unexpected#danger');
    }
    // Account is gone — drop the cookie and bounce to the marketing page.
    await clearSession();
    redirect('/');
  }

  return (
    <div style={pageStyle}>
      <header>
        <div className="ss-eyebrow" style={headerEyebrowStyle}>
          Account settings
        </div>
        <h1 style={titleStyle}>Preferences</h1>
        <p style={subtitleStyle}>
          Manage your StarStats Comm-Link, RSI handle ownership, sharing,
          and security.
        </p>
      </header>

      {status && (
        <div className="ss-alert ss-alert--ok" role="status">
          <span>{labelForStatus(status)}</span>
        </div>
      )}
      {error && (
        <div className="ss-alert ss-alert--danger" role="alert">
          <span>{labelForError(error)}</span>
        </div>
      )}

      {/* Account info */}
      <section className="ss-card">
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Account
          </div>
          <h2 style={cardTitleStyle}>Account info</h2>
        </header>
        <div style={cardBodyStyle}>
          <dl className="ss-kv">
            <dt>Comm-Link</dt>
            <dd>
              <span className="mono">{me.email}</span>{' '}
              {me.email_verified ? (
                <span className="ss-badge ss-badge--ok">
                  <span className="ss-badge-dot" />
                  Verified
                </span>
              ) : (
                <span className="ss-badge ss-badge--warn">
                  <span className="ss-badge-dot" />
                  Not verified
                </span>
              )}
            </dd>
            <dt>RSI handle</dt>
            <dd>
              <span className="mono">{me.claimed_handle}</span>{' '}
              {me.rsi_verified ? (
                <span className="ss-badge ss-badge--ok">
                  <span className="ss-badge-dot" />
                  Ownership proven
                </span>
              ) : (
                <span className="ss-badge ss-badge--warn">
                  Unverified
                </span>
              )}
            </dd>
            {me.pending_email && (
              <>
                <dt>Pending Comm-Link</dt>
                <dd>
                  <span className="mono">{me.pending_email}</span>{' '}
                  <span style={dimStyle}>· awaiting confirmation</span>
                </dd>
              </>
            )}
          </dl>
        </div>
      </section>

      {/* Email verification */}
      <section className="ss-card" id="verification">
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Comm-Link
          </div>
          <h2 style={cardTitleStyle}>Comm-Link verification</h2>
        </header>
        <div style={cardBodyStyle}>
          {me.email_verified ? (
            <p style={mutedStyle}>
              Your Comm-Link is verified. Nothing to do here.
            </p>
          ) : (
            <>
              <p style={mutedStyle}>
                We sent a verification link to{' '}
                <span className="mono" style={{ color: 'var(--fg)' }}>
                  {me.email}
                </span>
                . Didn&apos;t arrive? Resend it below.
              </p>
              <form action={resendAction} style={formStyle}>
                <button
                  type="submit"
                  className="ss-btn ss-btn--primary"
                  style={{ alignSelf: 'flex-start' }}
                >
                  Resend verification link
                </button>
              </form>
            </>
          )}
        </div>
      </section>

      {/* RSI handle ownership */}
      <section className="ss-card" id="rsi">
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Identity
          </div>
          <h2 style={cardTitleStyle}>RSI handle ownership</h2>
        </header>
        <div style={cardBodyStyle}>
          {me.rsi_verified ? (
            <>
              <p style={mutedStyle}>
                <span className="mono" style={{ color: 'var(--fg)' }}>
                  {me.claimed_handle}
                </span>{' '}
                is verified. Sharing, org access, and your public profile
                are unlocked.
              </p>
              <hr className="ss-rule" />
              <div className="ss-eyebrow">Citizen profile snapshot</div>
              {profileLoadFailed ? (
                <p style={mutedStyle}>
                  Couldn&apos;t load the snapshot. Refresh the page or try
                  again.
                </p>
              ) : profile ? (
                <p style={mutedStyle}>
                  Last refreshed:{' '}
                  <span className="mono" style={{ color: 'var(--fg)' }}>
                    {new Date(profile.captured_at).toLocaleString()}
                  </span>
                  .
                </p>
              ) : (
                <p style={mutedStyle}>
                  You haven&apos;t snapshotted your RSI profile yet.
                  Snapshots cache your display name, badges, bio, and
                  primary org so they show up on your dashboard and
                  public profile.
                </p>
              )}
              <div
                style={{
                  display: 'flex',
                  flexWrap: 'wrap',
                  gap: 8,
                  alignItems: 'center',
                }}
              >
                <form action={refreshProfileAction}>
                  <button type="submit" className="ss-btn ss-btn--ghost">
                    Refresh profile
                  </button>
                </form>
                <form action={refreshRsiOrgsAction}>
                  <button type="submit" className="ss-btn ss-btn--ghost">
                    Refresh orgs
                  </button>
                </form>
              </div>
            </>
          ) : rsiLoadFailed ? (
            <p style={mutedStyle}>
              Couldn&apos;t load the verification code right now. Refresh
              the page to try again.
            </p>
          ) : rsiState ? (
            <>
              <p style={mutedStyle}>
                Public profiles and shares display{' '}
                <span className="mono" style={{ color: 'var(--fg)' }}>
                  {me.claimed_handle}
                </span>{' '}
                as your name. To stop someone signing up as a handle that
                isn&apos;t theirs, we ask you to prove ownership by
                pasting a short code into your RSI public bio. Once
                verified, you can take the code back out — we only check
                it once.
              </p>
              <ol
                style={{
                  margin: 0,
                  paddingLeft: 20,
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 14,
                  color: 'var(--fg-muted)',
                  fontSize: 13,
                  lineHeight: 1.55,
                }}
              >
                <li>
                  Open{' '}
                  <a
                    href={`https://robertsspaceindustries.com/citizens/${encodeURIComponent(me.claimed_handle)}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{ color: 'var(--accent)' }}
                  >
                    your RSI public profile
                  </a>{' '}
                  and click <em>Edit Profile</em> → <em>Bio</em>.
                </li>
                <li>
                  Paste this code anywhere in the bio:
                  <div className="ss-secret" style={{ marginTop: 8 }}>
                    <code className="ss-secret-code mono">
                      {rsiState.code}
                    </code>
                  </div>
                  <small style={{ ...dimStyle, display: 'block', marginTop: 6 }}>
                    Expires{' '}
                    <span className="mono">
                      {rsiState.expires_at
                        ? new Date(rsiState.expires_at).toLocaleString()
                        : '(unknown)'}
                    </span>
                    . Save the bio in RSI before pressing the button below.
                  </small>
                </li>
                <li>
                  <form action={rsiCheckAction} style={formStyle}>
                    <button
                      type="submit"
                      className="ss-btn ss-btn--primary"
                      style={{ alignSelf: 'flex-start' }}
                    >
                      Check now
                    </button>
                  </form>
                </li>
              </ol>
            </>
          ) : (
            <p style={mutedStyle}>Loading verification state…</p>
          )}
        </div>
      </section>

      {/* Hangar sync status (read-only). The tray writes; the web
          shows the result so the user can confirm the link is alive
          without leaving the browser. */}
      <HangarCard snapshot={hangar} />

      {/* Sharing moved to /sharing in 0.0.4-beta. This stub keeps
          /settings#sharing anchor links working for old bookmarks
          and surfaces the new location for users who land here. */}
      <section className="ss-card" id="sharing">
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Sharing
          </div>
          <h2 style={cardTitleStyle}>Moved to its own page</h2>
        </header>
        <div style={cardBodyStyle}>
          <p style={mutedStyle}>
            Profile visibility, granted shares, and the list of people
            sharing with you now live at{' '}
            <Link href="/sharing" style={{ color: 'var(--accent)' }}>
              /sharing
            </Link>
            .
          </p>
        </div>
      </section>

      {/* Theme — clicking a swatch submits the form, which writes the
          ss-theme cookie and persists via PUT /v1/me/preferences. */}
      <section className="ss-card" id="theme">
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Theme
          </div>
          <h2 style={cardTitleStyle}>Appearance</h2>
        </header>
        <div style={cardBodyStyle}>
          <p style={mutedStyle}>
            Themes change accent and surface tints. Type, spacing, and
            component shapes are identical across all four. Your choice
            follows you across devices.
          </p>
          <form action={themeAction} style={{ margin: 0 }}>
            <div data-rspgrid="4" style={themeGridStyle}>
              {THEME_PREVIEWS.map((t) => (
                <ThemeSwatchButton
                  key={t.id}
                  theme={t}
                  active={t.id === activeTheme}
                />
              ))}
            </div>
          </form>
        </div>
      </section>

      {/* Password — full-width row above the inline Security/2FA card.
          The 2FA wizard is now absorbed into <SecuritySection> below
          (audit v2 §09: inline 2FA wizard into Settings → Security),
          so password no longer shares a 2-col row with it. */}
      <section className="ss-card" id="password">
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Security
          </div>
          <h2 style={cardTitleStyle}>Change password</h2>
        </header>
        <div style={cardBodyStyle}>
          <form action={passwordAction} style={formStyle}>
            <label className="ss-label">
              <span className="ss-label-text">Current password</span>
              <input
                className="ss-input"
                type="password"
                name="current_password"
                required
                autoComplete="current-password"
              />
            </label>
            <label className="ss-label">
              <span className="ss-label-text">New password</span>
              <input
                className="ss-input"
                type="password"
                name="new_password"
                required
                minLength={12}
                autoComplete="new-password"
              />
              <small style={dimStyle}>At least 12 characters.</small>
            </label>
            <button
              type="submit"
              className="ss-btn ss-btn--primary"
              style={{ alignSelf: 'flex-start' }}
            >
              Update password
            </button>
          </form>
        </div>
      </section>

      {/* Inline two-factor wizard. Replaces the standalone /settings/2fa
          route per audit v2 §07 (absorb) and §09 (inline into Settings).
          Anchored at #security so the legacy redirect from /settings/2fa
          lands on this card. */}
      <SecuritySection me={me} />

      {/* Change Comm-Link */}
      <section className="ss-card" id="email">
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Account lifecycle
          </div>
          <h2 style={cardTitleStyle}>Change sign-in Comm-Link</h2>
        </header>
        <div style={cardBodyStyle}>
          {me.pending_email ? (
            <p style={mutedStyle}>
              We sent a confirmation link to{' '}
              <span className="mono" style={{ color: 'var(--fg)' }}>
                {me.pending_email}
              </span>
              . Click it from that inbox to switch your sign-in Comm-Link.
              The link expires in 24 hours. Submitting this form again
              replaces the pending address.
            </p>
          ) : (
            <p style={mutedStyle}>
              We&apos;ll send a confirmation link to the new address; your
              sign-in Comm-Link only changes after you click it. Your
              current address (
              <span className="mono" style={{ color: 'var(--fg)' }}>
                {me.email}
              </span>
              ) stays active until then.
            </p>
          )}
          <form action={emailChangeAction} style={formRowEndStyle}>
            <label className="ss-label" style={{ flex: 1, minWidth: 220 }}>
              <span className="ss-label-text">New Comm-Link</span>
              <input
                className="ss-input"
                type="email"
                name="new_email"
                required
                autoComplete="email"
                spellCheck={false}
                placeholder="new@example.com"
              />
            </label>
            <button type="submit" className="ss-btn ss-btn--primary">
              {me.pending_email
                ? 'Replace pending change'
                : 'Send confirmation link'}
            </button>
          </form>
        </div>
      </section>

      {/* Danger zone */}
      <section className="ss-card" id="danger" style={dangerCardStyle}>
        <header style={cardHeaderStyle}>
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Danger zone
          </div>
          <h2 style={dangerCardTitleStyle}>Delete account</h2>
        </header>
        <form
          id="delete-account-form"
          action={deleteAction}
          style={{ margin: 0 }}
        >
          <div style={cardBodyStyle}>
            <p style={mutedStyle}>
              Deleting your account is permanent. Your account record,
              paired devices, and active shares are removed. Your
              ingested game events are pseudonymised — the row count is
              preserved so anyone you shared with keeps a coherent
              timeline, but the data is no longer linked to you or your
              RSI handle. To confirm, type your RSI handle (
              <span className="mono" style={{ color: 'var(--fg)' }}>
                {me.claimed_handle}
              </span>
              ) below.
            </p>
            <label className="ss-label">
              <span className="ss-label-text">
                Type your handle to confirm
              </span>
              <input
                className="ss-input"
                type="text"
                name="confirm_handle"
                required
                autoComplete="off"
                spellCheck={false}
                placeholder={me.claimed_handle}
                style={monoStyle}
              />
            </label>
          </div>
          <hr className="ss-rule" />
          <div style={cardFooterStyle}>
            <button type="submit" className="ss-btn ss-btn--danger">
              Delete my account
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}

function labelForStatus(code: string): string {
  switch (code) {
    case 'resent':
      return 'Verification Comm-Link sent. Check your inbox.';
    case 'already_verified':
      return 'Your Comm-Link is already verified — no message was sent.';
    case 'password_changed':
      return 'Password updated.';
    case 'visibility_public':
      return 'Your profile is now public.';
    case 'visibility_private':
      return 'Your profile is now private.';
    case 'share_added':
      return 'Access granted.';
    case 'share_revoked':
      return 'Access revoked.';
    case 'org_share_added':
      return 'Org access granted.';
    case 'org_share_revoked':
      return 'Org access revoked.';
    case 'email_change_sent':
      return 'Confirmation link sent. Check the new inbox to finish the change.';
    case 'rsi_verified':
      return 'RSI handle verified. You can take the code back out of your bio now.';
    case 'profile_refreshed':
      return 'Profile snapshot refreshed.';
    case 'orgs_refreshed':
      return 'Org snapshot refreshed.';
    case 'theme_updated':
      return 'Theme updated.';
    case 'totp_enabled':
      return "Two-factor enabled. Save your recovery codes below — you won't see them again.";
    case 'totp_ack':
      return 'Recovery codes acknowledged.';
    case 'totp_disabled':
      return 'Two-factor disabled.';
    case 'totp_regenerated':
      return 'New recovery codes generated. Save them — the old set is gone.';
    default:
      return 'Done.';
  }
}

function labelForError(code: string): string {
  switch (code) {
    case 'invalid_credentials':
      return 'Current password is incorrect.';
    case 'password_too_short':
      return 'New password must be at least 12 characters.';
    case 'confirm_mismatch':
      return "That handle doesn't match. Account was not deleted.";
    case 'recipient_not_found':
      return "We couldn't find a StarStats user with that handle.";
    case 'cannot_share_with_self':
      return "You can't share your stats with yourself.";
    case 'invalid_recipient_handle':
      return 'That handle looks invalid. Use letters, digits, _ or -.';
    case 'invalid_org_slug':
      return 'That org slug looks invalid.';
    case 'org_not_found':
      return "We couldn't find an org with that slug.";
    case 'spicedb_unavailable':
      return 'Sharing is temporarily unavailable. Please try again shortly.';
    case 'rsi_handle_not_verified':
      return "Verify your RSI handle (above) before sharing — public profiles and shares display your handle, so we need to confirm it's yours.";
    case 'invalid_email':
      return 'That Comm-Link address looks invalid.';
    case 'email_taken':
      return 'That Comm-Link is already in use by another account.';
    case 'rsi_code_not_in_bio':
      return "We couldn't find the code in your RSI bio. Make sure you saved the bio after pasting it.";
    case 'rsi_handle_not_found':
      return "RSI doesn't have a public profile for that handle. Check the spelling matches your RSI account exactly.";
    case 'rsi_code_expired':
      return 'The verification code expired. Refresh this page to get a fresh one.';
    case 'rsi_unavailable':
      return 'RSI is unreachable right now. Please try again in a few minutes.';
    case 'rsi_unknown':
      return 'Something went wrong checking your bio. Please try again.';
    case 'refresh_too_soon':
      return 'Profile was just refreshed — please wait a few minutes before refreshing again.';
    case 'orgs_refresh_too_soon':
      return 'Orgs were just refreshed — please wait a few minutes before refreshing again.';
    case 'invalid_theme':
      return "That theme isn't recognised. Pick one of the four shown.";
    case 'invalid_code':
      return "That authentication code didn't match. Check the time on your device and try again.";
    case 'no_setup':
      return 'Start two-factor setup before trying to confirm.';
    case 'already_enabled':
      return 'Two-factor is already enabled on this account.';
    case 'not_enabled':
      return "Two-factor isn't enabled on this account.";
    case 'unexpected':
      return 'Something went wrong. Please try again.';
    default:
      return `Couldn't complete that action (${code}).`;
  }
}
