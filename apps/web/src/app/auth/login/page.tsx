import { redirect } from 'next/navigation';
import { ApiCallError, getMe, login } from '@/lib/api';
import { logger } from '@/lib/logger';
import { authAttemptsTotal } from '@/lib/metrics';
import { setSession } from '@/lib/session';
import Link from 'next/link';

interface SearchParams {
  error?: string;
}

const mainStyle: React.CSSProperties = {
  maxWidth: 'none',
  padding: '32px 24px 60px',
  display: 'grid',
  placeItems: 'start center',
};

const cardStyle: React.CSSProperties = {
  width: '100%',
  maxWidth: 420,
  display: 'flex',
  flexDirection: 'column',
  gap: 20,
};

const titleStyle: React.CSSProperties = {
  margin: '8px 0 0',
  fontSize: 28,
  fontWeight: 600,
  letterSpacing: '-0.02em',
};

const subtitleStyle: React.CSSProperties = {
  margin: 0,
  color: 'var(--fg-muted)',
  fontSize: 14,
  lineHeight: 1.55,
};

const formStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 14,
  margin: 0,
};

const actionsStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  marginTop: 6,
};

const linksStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  fontSize: 13,
};

export default async function LoginPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const { error } = await props.searchParams;

  async function action(formData: FormData) {
    'use server';

    const email = String(formData.get('email') ?? '').trim();
    const password = String(formData.get('password') ?? '');

    let auth;
    try {
      auth = await login({ email, password });
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        authAttemptsTotal.inc({ action: 'login', outcome: 'invalid_credentials' });
        logger.info('login rejected: invalid credentials');
        redirect('/auth/login?error=invalid_credentials');
      }
      authAttemptsTotal.inc({ action: 'login', outcome: 'unexpected' });
      logger.error({ err: e }, 'login failed unexpectedly');
      redirect('/auth/login?error=unexpected');
    }

    // 2FA fork. The interim token is short-lived and only valid for
    // /v1/auth/totp/verify-login; we shuttle it through the URL
    // because storing it as a session cookie would defeat the
    // "you haven't actually logged in yet" semantics.
    if (auth.totp_required) {
      authAttemptsTotal.inc({ action: 'login', outcome: 'totp_required' });
      redirect(
        `/auth/totp-verify?interim=${encodeURIComponent(auth.token)}`,
      );
    }

    // Hydrate emailVerified into the session cookie via /v1/auth/me
    // so the layout banner has it without a per-request API call.
    // Failure here is non-fatal: we degrade to `false` so the user
    // sees the verify banner, which is the safer default.
    let emailVerified = false;
    let staffRoles: string[] = [];
    try {
      const me = await getMe(auth.token);
      emailVerified = me.email_verified;
      staffRoles = me.staff_roles ?? [];
    } catch (meErr) {
      logger.warn(
        { err: meErr },
        'getMe after login failed; defaulting emailVerified=false',
      );
    }
    await setSession({
      token: auth.token,
      userId: auth.user_id,
      claimedHandle: auth.claimed_handle,
      emailVerified,
      staffRoles,
    });
    authAttemptsTotal.inc({ action: 'login', outcome: 'success' });
    logger.info({ user_id: auth.user_id }, 'login success');
    redirect('/devices');
  }

  return (
    <main className="auth" style={mainStyle}>
      <div style={cardStyle}>
        <span className="ss-eyebrow">Sign in</span>
        <h1 style={titleStyle}>Sign in.</h1>
        <p style={subtitleStyle}>
          Welcome back. Use your Comm-Link and password.
        </p>

        {error === 'invalid_credentials' && (
          <div className="ss-alert ss-alert--danger" role="alert">
            Comm-Link or password is incorrect.
          </div>
        )}
        {error === 'unexpected' && (
          <div className="ss-alert ss-alert--danger" role="alert">
            Something went wrong. Please try again.
          </div>
        )}
        {error === 'interim_required' && (
          <div className="ss-alert ss-alert--warn" role="alert">
            Sign-in session expired. Start over.
          </div>
        )}

        <form action={action} style={formStyle}>
          <label className="ss-label">
            <span className="ss-label-text">Comm-Link</span>
            <input
              className="ss-input"
              type="email"
              name="email"
              required
              autoComplete="email"
              spellCheck={false}
              placeholder="you@example.com"
            />
          </label>

          <label className="ss-label">
            <span className="ss-label-text">Password</span>
            <input
              className="ss-input"
              type="password"
              name="password"
              required
              autoComplete="current-password"
            />
          </label>

          <div style={actionsStyle}>
            <button type="submit" className="ss-btn ss-btn--primary">
              Sign in
            </button>
            <Link href="/auth/magic-link" className="ss-btn ss-btn--ghost">
              Send magic link instead
            </Link>
          </div>
        </form>

        <hr className="ss-rule" />

        <div style={linksStyle}>
          <Link href="/auth/forgot-password" style={{ color: 'var(--fg-muted)' }}>
            Forgot your password?
          </Link>
          <span style={{ color: 'var(--fg-muted)' }}>
            New here?{' '}
            <Link href="/auth/signup" style={{ color: 'var(--accent)' }}>
              Create an account
            </Link>
            .
          </span>
        </div>
      </div>
    </main>
  );
}
