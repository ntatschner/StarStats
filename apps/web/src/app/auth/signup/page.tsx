import { redirect } from 'next/navigation';
import { ApiCallError, getMe, signup } from '@/lib/api';
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
  maxWidth: 440,
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

const fineprintStyle: React.CSSProperties = {
  fontSize: 12,
  color: 'var(--fg-dim)',
  lineHeight: 1.5,
  margin: 0,
};

export default async function SignupPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const { error } = await props.searchParams;

  async function action(formData: FormData) {
    'use server';

    const email = String(formData.get('email') ?? '').trim();
    const password = String(formData.get('password') ?? '');
    const claimedHandle = String(formData.get('claimed_handle') ?? '').trim();

    try {
      const auth = await signup({
        email,
        password,
        claimed_handle: claimedHandle,
      });
      // Brand-new accounts are nearly always unverified, but fetch
      // /v1/auth/me to be honest about the source of truth. Failure
      // here is non-fatal — degrade to `false` so the verify banner
      // shows up.
      let emailVerified = false;
      try {
        const me = await getMe(auth.token);
        emailVerified = me.email_verified;
      } catch (meErr) {
        logger.warn({ err: meErr }, 'getMe after signup failed; defaulting emailVerified=false');
      }
      await setSession({
        token: auth.token,
        userId: auth.user_id,
        claimedHandle: auth.claimed_handle,
        emailVerified,
      });
      authAttemptsTotal.inc({ action: 'signup', outcome: 'success' });
      logger.info({ user_id: auth.user_id }, 'signup success');
    } catch (e) {
      if (e instanceof ApiCallError) {
        authAttemptsTotal.inc({ action: 'signup', outcome: e.body.error });
        logger.info({ reason: e.body.error }, 'signup rejected');
        redirect(`/auth/signup?error=${encodeURIComponent(e.body.error)}`);
      }
      authAttemptsTotal.inc({ action: 'signup', outcome: 'unexpected' });
      logger.error({ err: e }, 'signup failed unexpectedly');
      redirect('/auth/signup?error=unexpected');
    }
    redirect('/devices');
  }

  return (
    <main className="auth" style={mainStyle}>
      <div style={cardStyle}>
        <span className="ss-eyebrow">Create account</span>
        <h1 style={titleStyle}>Create your account.</h1>
        <p style={subtitleStyle}>
          Comm-Link plus a password gets you a hangar. You can verify your RSI
          handle later.
        </p>

        {error && (
          <div className="ss-alert ss-alert--danger" role="alert">
            {labelForError(error)}
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
              minLength={12}
              autoComplete="new-password"
            />
            <small style={{ color: 'var(--fg-dim)', fontSize: 12 }}>
              At least 12 characters.
            </small>
          </label>

          <label className="ss-label">
            <span className="ss-label-text">RSI handle</span>
            <input
              className="ss-input"
              type="text"
              name="claimed_handle"
              required
              autoComplete="username"
              placeholder="TheCodeSaiyan"
              spellCheck={false}
            />
            <small style={{ color: 'var(--fg-dim)', fontSize: 12 }}>
              The handle that appears in your Game.log.
            </small>
          </label>

          <button
            type="submit"
            className="ss-btn ss-btn--primary"
            style={{ marginTop: 6 }}
          >
            Create account
          </button>
        </form>

        <p style={fineprintStyle}>
          By creating an account you acknowledge our{' '}
          <Link href="/privacy" style={{ color: 'var(--fg-muted)' }}>
            Privacy Policy
          </Link>
          . We process your Comm-Link for authentication and account recovery
          (contract performance, GDPR Art. 6(1)(b)) and your RSI handle to tag
          the game events you choose to upload.
        </p>

        <hr className="ss-rule" />

        <p
          style={{
            margin: 0,
            fontSize: 13,
            color: 'var(--fg-muted)',
          }}
        >
          Already have an account?{' '}
          <Link href="/auth/login" style={{ color: 'var(--accent)' }}>
            Sign in
          </Link>
          .
        </p>
      </div>
    </main>
  );
}

function labelForError(code: string): string {
  switch (code) {
    case 'invalid_email':
      return "That Comm-Link doesn't look right — make sure it has @ and a domain.";
    case 'password_too_short':
      return 'Password must be at least 12 characters.';
    case 'missing_handle':
      return 'RSI handle is required.';
    case 'email_taken':
      return 'An account with that Comm-Link already exists.';
    case 'handle_taken':
      return 'Someone else already claimed that RSI handle.';
    default:
      return "Something went wrong. Please try again, or check the URL bar's error code.";
  }
}
