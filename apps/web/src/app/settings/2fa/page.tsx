import Link from 'next/link';
import { redirect } from 'next/navigation';
import { cookies } from 'next/headers';
import {
  ApiCallError,
  getMe,
  totpConfirm,
  totpDisable,
  totpRegenerateRecovery,
  totpSetup,
  type MeResponse,
  type TotpSetupResponse,
} from '@/lib/api';
import { logger } from '@/lib/logger';
import { getSession } from '@/lib/session';

interface SearchParams {
  status?: string;
  error?: string;
}

const SETUP_COOKIE = 'totp-setup';
const RECOVERY_COOKIE = 'totp-recovery';
// Setup cookie holds the base32 secret + provisioning URI so the
// confirm-code page can re-render the QR after a refresh without
// pinging the server again. Ten minutes is plenty for a user to
// install the QR, type a code, and hit submit; if they take longer
// they can just hit "Cancel" and start over.
const SETUP_COOKIE_TTL_SECS = 10 * 60;
// Recovery cookie carries the freshly-minted codes through exactly
// one render. As soon as the user clicks "I've saved them" we drop
// the cookie. Two minutes is a hard ceiling — if a window sits open
// any longer the codes shouldn't still be there.
const RECOVERY_COOKIE_TTL_SECS = 2 * 60;

interface SetupCookiePayload {
  secret_base32: string;
  provisioning_uri: string;
  account_label: string;
}

/**
 * Two-factor authentication management.
 *
 * The wizard is driven by `me.totp_enabled` plus two short-lived
 * cookies. The cookies hold the only data we can't recover from the
 * server on the next render:
 *
 *   - `totp-setup`: the base32 secret + provisioning URI between
 *     "user clicked Enable" and "user typed the first valid code".
 *     The server keeps the encrypted secret in the user row, but it
 *     can't hand back the plaintext (we don't want a "leak my secret"
 *     endpoint), so we cache the setup payload in an httpOnly cookie.
 *
 *   - `totp-recovery`: the 10 plaintext recovery codes returned by
 *     confirm or regenerate. They're Argon2-hashed on the server,
 *     so this is the only window the user has to read them. We show
 *     them once and clear the cookie on acknowledgement.
 *
 * Both cookies are httpOnly + sameSite=lax + path-scoped so they
 * never leak to client-side JS or other origins.
 */
export default async function TwoFactorPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/settings/2fa');

  const { status, error } = await props.searchParams;

  let me: MeResponse;
  try {
    me = await getMe(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/settings/2fa');
    }
    throw e;
  }

  const jar = await cookies();
  const setupRaw = jar.get(SETUP_COOKIE)?.value;
  const recoveryRaw = jar.get(RECOVERY_COOKIE)?.value;

  // Parse cookies defensively. A tampered or stale cookie should
  // fall back to the "no cookie" branch rather than throwing.
  let setup: SetupCookiePayload | null = null;
  if (setupRaw) {
    try {
      setup = JSON.parse(setupRaw) as SetupCookiePayload;
    } catch {
      setup = null;
    }
  }
  let recoveryCodes: string[] | null = null;
  if (recoveryRaw) {
    try {
      const parsed = JSON.parse(recoveryRaw) as unknown;
      if (Array.isArray(parsed) && parsed.every((c) => typeof c === 'string')) {
        recoveryCodes = parsed;
      }
    } catch {
      recoveryCodes = null;
    }
  }

  async function setupAction() {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings/2fa');
    let resp: TotpSetupResponse;
    try {
      resp = await totpSetup(session.token);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        redirect('/auth/login?next=/settings/2fa');
      }
      if (e instanceof ApiCallError && e.status === 409) {
        // Already enabled — fall through to the manage view.
        redirect('/settings/2fa');
      }
      logger.error({ err: e }, 'totp setup failed');
      redirect('/settings/2fa?error=unexpected');
    }
    const payload: SetupCookiePayload = {
      secret_base32: resp.secret_base32,
      provisioning_uri: resp.provisioning_uri,
      account_label: resp.account_label,
    };
    const jar = await cookies();
    jar.set({
      name: SETUP_COOKIE,
      value: JSON.stringify(payload),
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'lax',
      path: '/settings/2fa',
      maxAge: SETUP_COOKIE_TTL_SECS,
    });
    redirect('/settings/2fa');
  }

  async function cancelSetupAction() {
    'use server';
    const jar = await cookies();
    jar.delete(SETUP_COOKIE);
    redirect('/settings/2fa');
  }

  async function confirmAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings/2fa');
    const code = String(formData.get('code') ?? '').trim();
    if (code === '') {
      redirect('/settings/2fa?error=invalid_code');
    }
    let codes: string[];
    try {
      const resp = await totpConfirm(session.token, { code });
      codes = resp.recovery_codes;
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) redirect('/auth/login?next=/settings/2fa');
        if (e.status === 400) redirect('/settings/2fa?error=no_setup');
        if (e.status === 409) redirect('/settings/2fa?error=already_enabled');
        if (e.status === 422) redirect('/settings/2fa?error=invalid_code');
      }
      logger.error({ err: e }, 'totp confirm failed');
      redirect('/settings/2fa?error=unexpected');
    }
    const jar = await cookies();
    jar.delete(SETUP_COOKIE);
    jar.set({
      name: RECOVERY_COOKIE,
      value: JSON.stringify(codes),
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'lax',
      path: '/settings/2fa',
      maxAge: RECOVERY_COOKIE_TTL_SECS,
    });
    redirect('/settings/2fa?status=enabled');
  }

  async function acknowledgeRecoveryAction() {
    'use server';
    const jar = await cookies();
    jar.delete(RECOVERY_COOKIE);
    redirect('/settings/2fa?status=ack');
  }

  async function disableAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings/2fa');
    const password = String(formData.get('password') ?? '');
    try {
      await totpDisable(session.token, { password });
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) {
          // 401 on this endpoint means the *password* check failed,
          // not the bearer — the bearer is required to even reach the
          // handler. Surface as invalid_credentials.
          redirect('/settings/2fa?error=invalid_credentials');
        }
        if (e.status === 409) redirect('/settings/2fa?error=not_enabled');
      }
      logger.error({ err: e }, 'totp disable failed');
      redirect('/settings/2fa?error=unexpected');
    }
    const jar = await cookies();
    jar.delete(SETUP_COOKIE);
    jar.delete(RECOVERY_COOKIE);
    redirect('/settings/2fa?status=disabled');
  }

  async function regenerateAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/settings/2fa');
    const password = String(formData.get('password') ?? '');
    let codes: string[];
    try {
      const resp = await totpRegenerateRecovery(session.token, { password });
      codes = resp.recovery_codes;
    } catch (e) {
      if (e instanceof ApiCallError) {
        if (e.status === 401) {
          redirect('/settings/2fa?error=invalid_credentials');
        }
        if (e.status === 409) redirect('/settings/2fa?error=not_enabled');
      }
      logger.error({ err: e }, 'totp regenerate recovery failed');
      redirect('/settings/2fa?error=unexpected');
    }
    const jar = await cookies();
    jar.set({
      name: RECOVERY_COOKIE,
      value: JSON.stringify(codes),
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'lax',
      path: '/settings/2fa',
      maxAge: RECOVERY_COOKIE_TTL_SECS,
    });
    redirect('/settings/2fa?status=regenerated');
  }

  // Wizard step indicator. The four states are:
  //   1. Off          (explainer / begin enrolment)
  //   2. Setup        (QR + secret + verify code)
  //   3. Recovery     (10 codes shown once)
  //   4. Manage       (regen / disable)
  //
  // The legacy state machine still drives the conditional render
  // below — `currentStep` is just a header decoration so users see
  // where they are in the flow.
  let currentStep: 1 | 2 | 3 | 4 = 1;
  if (recoveryCodes) {
    currentStep = 3;
  } else if (setup && !me.totp_enabled) {
    currentStep = 2;
  } else if (me.totp_enabled) {
    currentStep = 4;
  }

  return (
    <main className="dashboard">
      <header className="dashboard__header">
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Security · Two-factor
        </div>
        <h1>Two-factor authentication</h1>
        <p className="muted">
          Add a second factor to your sign-in. Once enabled, every
          login asks for a 6-digit authentication code from your
          authenticator app.{' '}
          <Link href="/settings">Back to settings</Link>.
        </p>
        <WizardSteps current={currentStep} />
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

      {/* Step 3 — Recovery codes screen, shown right after enable or regenerate. */}
      {recoveryCodes && (
        <section className="ss-card ss-card-pad">
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Step 3 · Recovery codes
          </div>
          <h2 style={{ margin: '0 0 8px', fontSize: 20, fontWeight: 600, letterSpacing: '-0.01em' }}>
            Save these somewhere safe
          </h2>
          <p className="muted" style={{ marginTop: 0 }}>
            Store these somewhere safe. Each code works once. They get
            you back in if you lose your authenticator app — we
            can&apos;t show them again.
          </p>
          <div
            className="ss-secret"
            style={{
              padding: 16,
              alignItems: 'flex-start',
              flexDirection: 'column',
              gap: 6,
            }}
          >
            <div
              className="mono"
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
                gap: '6px 24px',
                width: '100%',
                fontSize: 13,
                lineHeight: 1.7,
                letterSpacing: '0.04em',
                color: 'var(--fg)',
              }}
            >
              {recoveryCodes.map((code, i) => (
                <span key={code}>
                  <span style={{ color: 'var(--fg-dim)', marginRight: 8 }}>
                    {String(i + 1).padStart(2, '0')}
                  </span>
                  {code}
                </span>
              ))}
            </div>
          </div>
          <hr className="ss-rule" style={{ margin: '16px 0' }} />
          <form action={acknowledgeRecoveryAction} className="form">
            <button type="submit" className="ss-btn ss-btn--primary">
              I&apos;ve saved them
            </button>
          </form>
        </section>
      )}

      {/* Step 2 — Setup-in-progress: QR + manual secret + verify code. */}
      {!recoveryCodes && !me.totp_enabled && setup && (
        <section className="ss-card ss-card-pad">
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Step 2 · Scan and confirm
          </div>
          <h2 style={{ margin: '0 0 8px', fontSize: 20, fontWeight: 600, letterSpacing: '-0.01em' }}>
            Pair your authenticator
          </h2>
          <p className="muted" style={{ marginTop: 0 }}>
            Scan in your authenticator app, or enter the secret
            manually. The label{' '}
            <span className="mono">{setup.account_label}</span> is what
            appears in the app.
          </p>

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'minmax(0, 200px) 1fr',
              gap: 18,
              alignItems: 'start',
              marginTop: 4,
            }}
          >
            {/* QR code — rendered by qrserver.com from the provisioning URI.
                Same approach as the legacy page, just visually framed. */}
            <div
              style={{
                background: 'var(--bg-elev)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--r-sm)',
                padding: 12,
                display: 'grid',
                placeItems: 'center',
              }}
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={`https://api.qrserver.com/v1/create-qr-code/?size=200x200&margin=0&data=${encodeURIComponent(setup.provisioning_uri)}`}
                alt="Authenticator QR code"
                width={176}
                height={176}
                style={{ display: 'block', background: 'var(--fg)', borderRadius: 4 }}
              />
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <span className="ss-label-text">Manual secret</span>
                <div className="ss-secret" style={{ marginTop: 6 }}>
                  <span className="ss-secret-code mono">
                    {setup.secret_base32}
                  </span>
                </div>
                <small style={{ color: 'var(--fg-dim)', display: 'block', marginTop: 6 }}>
                  SHA-1 · 6 digits · 30s period
                </small>
              </div>

              <div>
                <span className="ss-label-text">Provisioning URI</span>
                <div className="ss-secret" style={{ marginTop: 6 }}>
                  <span
                    className="ss-secret-code mono"
                    style={{ fontSize: 12, letterSpacing: 0 }}
                  >
                    {setup.provisioning_uri}
                  </span>
                </div>
              </div>
            </div>
          </div>

          <hr className="ss-rule" style={{ margin: '20px 0' }} />

          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Verify
          </div>
          <h3 style={{ margin: '0 0 8px', fontSize: 16, fontWeight: 600 }}>
            Enter the authentication code
          </h3>
          <p className="muted" style={{ marginTop: 0 }}>
            Type the 6-digit code your app currently displays. It
            refreshes every 30 seconds.
          </p>
          <form action={confirmAction} className="form">
            <label>
              <span className="ss-label-text">Authentication code</span>
              <input
                className="ss-input mono"
                type="text"
                name="code"
                required
                inputMode="numeric"
                autoComplete="one-time-code"
                pattern="[0-9]{6}"
                maxLength={6}
                placeholder="123456"
                spellCheck={false}
                style={{
                  letterSpacing: '0.4em',
                  fontSize: 18,
                  textAlign: 'center',
                  maxWidth: 220,
                }}
              />
            </label>
            <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
              <button type="submit" className="ss-btn ss-btn--primary">
                Verify
              </button>
            </div>
          </form>

          <hr className="ss-rule" style={{ margin: '16px 0' }} />
          <form action={cancelSetupAction} className="form">
            <button type="submit" className="ss-btn ss-btn--ghost">
              Cancel setup
            </button>
          </form>
        </section>
      )}

      {/* Step 4 — Manage view, 2FA already enabled. */}
      {!recoveryCodes && me.totp_enabled && (
        <>
          <section className="ss-card ss-card-pad">
            <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
              Step 4 · Active
            </div>
            <h2 style={{ margin: '0 0 8px', fontSize: 20, fontWeight: 600, letterSpacing: '-0.01em' }}>
              Two-factor is on
            </h2>
            <p className="muted" style={{ marginTop: 0 }}>
              Sign-ins from this account require an authentication
              code from your authenticator app, or a one-shot recovery
              code if you&apos;ve lost the app.
            </p>
          </section>

          <section className="ss-card ss-card-pad">
            <h2 style={{ margin: '0 0 8px', fontSize: 18, fontWeight: 600, letterSpacing: '-0.01em' }}>
              Regenerate recovery codes
            </h2>
            <p className="muted" style={{ marginTop: 0 }}>
              Burn the old set and mint 10 fresh codes. Useful if you
              think the old set leaked, or if you&apos;ve used most
              of them. Re-enter your password to confirm.
            </p>
            <form action={regenerateAction} className="form">
              <label>
                <span className="ss-label-text">Current password</span>
                <input
                  className="ss-input"
                  type="password"
                  name="password"
                  required
                  autoComplete="current-password"
                />
              </label>
              <button type="submit" className="ss-btn ss-btn--ghost">
                Generate new codes
              </button>
            </form>
          </section>

          <section className="ss-card ss-card-pad">
            <div className="ss-eyebrow" style={{ marginBottom: 6, color: 'var(--danger)' }}>
              Danger zone
            </div>
            <h2 style={{ margin: '0 0 8px', fontSize: 18, fontWeight: 600, letterSpacing: '-0.01em' }}>
              Disable two-factor
            </h2>
            <p className="muted" style={{ marginTop: 0 }}>
              Removes your authenticator secret and burns all recovery
              codes. Your account drops back to password-only sign-in.
              Re-enter your password to confirm.
            </p>
            <form action={disableAction} className="form">
              <label>
                <span className="ss-label-text">Current password</span>
                <input
                  className="ss-input"
                  type="password"
                  name="password"
                  required
                  autoComplete="current-password"
                />
              </label>
              <button type="submit" className="ss-btn ss-btn--danger">
                Turn off 2FA
              </button>
            </form>
          </section>
        </>
      )}

      {/* Step 1 — Explainer / no setup in flight, 2FA off. */}
      {!recoveryCodes && !me.totp_enabled && !setup && (
        <section className="ss-card ss-card-pad">
          <div className="ss-eyebrow" style={{ marginBottom: 6 }}>
            Step 1 · Get started
          </div>
          <h2 style={{ margin: '0 0 8px', fontSize: 20, fontWeight: 600, letterSpacing: '-0.01em' }}>
            Two-factor is off
          </h2>
          <p className="muted" style={{ marginTop: 0 }}>
            Right now your account is protected by your password
            alone. Two-factor authentication adds a second check —
            a 6-digit code from an authenticator app on your phone
            (1Password, Authy, Google Authenticator, etc.). Anyone
            who steals your password still can&apos;t sign in
            without that device.
          </p>
          <hr className="ss-rule" style={{ margin: '16px 0' }} />
          <form action={setupAction} className="form">
            <button type="submit" className="ss-btn ss-btn--primary">
              Begin enrolment
            </button>
          </form>
        </section>
      )}
    </main>
  );
}

/**
 * Compact 4-step indicator that mirrors the prototype's wizard
 * header. Status is purely visual — completion is derived from the
 * caller's `current` step.
 */
function WizardSteps({ current }: { current: 1 | 2 | 3 | 4 }) {
  const steps: Array<{ n: 1 | 2 | 3 | 4; label: string }> = [
    { n: 1, label: 'Get started' },
    { n: 2, label: 'Scan and confirm' },
    { n: 3, label: 'Save recovery codes' },
    { n: 4, label: 'Active' },
  ];

  return (
    <ol
      style={{
        listStyle: 'none',
        padding: 0,
        margin: '14px 0 0',
        display: 'flex',
        flexWrap: 'wrap',
        gap: 8,
      }}
    >
      {steps.map((s) => {
        const isDone = s.n < current;
        const isActive = s.n === current;
        const dotBg = isDone
          ? 'var(--accent)'
          : isActive
            ? 'var(--bg-elev)'
            : 'var(--bg-elev)';
        const dotColor = isDone
          ? 'var(--accent-fg)'
          : isActive
            ? 'var(--accent)'
            : 'var(--fg-dim)';
        const dotBorder = isActive
          ? '1px solid var(--accent)'
          : '1px solid var(--border)';
        const labelColor = isActive
          ? 'var(--fg)'
          : isDone
            ? 'var(--fg-muted)'
            : 'var(--fg-dim)';

        return (
          <li
            key={s.n}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              padding: '4px 10px 4px 4px',
              border: '1px solid var(--border)',
              borderRadius: 999,
              background: 'var(--bg-elev)',
            }}
          >
            <span
              style={{
                width: 20,
                height: 20,
                borderRadius: 999,
                background: dotBg,
                color: dotColor,
                border: dotBorder,
                display: 'grid',
                placeItems: 'center',
                fontSize: 11,
                fontFamily: 'var(--font-mono)',
                fontWeight: 600,
              }}
            >
              {s.n}
            </span>
            <span
              style={{
                fontSize: 12,
                color: labelColor,
                letterSpacing: '0.01em',
              }}
            >
              {s.label}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

function labelForStatus(code: string): string {
  switch (code) {
    case 'enabled':
      return 'Two-factor enabled. Save your recovery codes below — you won\'t see them again.';
    case 'ack':
      return 'Recovery codes acknowledged.';
    case 'disabled':
      return 'Two-factor disabled.';
    case 'regenerated':
      return 'New recovery codes generated. Save them — the old set is gone.';
    default:
      return 'Done.';
  }
}

function labelForError(code: string): string {
  switch (code) {
    case 'invalid_code':
      return 'That authentication code didn\'t match. Check the time on your device and try again.';
    case 'invalid_credentials':
      return 'Password is incorrect.';
    case 'no_setup':
      return 'Start two-factor setup before trying to confirm.';
    case 'already_enabled':
      return 'Two-factor is already enabled on this account.';
    case 'not_enabled':
      return 'Two-factor isn\'t enabled on this account.';
    case 'unexpected':
      return 'Something went wrong. Please try again.';
    default:
      return `Couldn't complete that action (${code}).`;
  }
}
