/**
 * StarStats — auth screens (login, magic link sent, TOTP verify).
 *
 * These render WITHOUT the app shell — they're a centred single-card
 * layout against the bg, with the wordmark anchored top-left.
 */

const AuthFrame = ({ children, eyebrow, go }) => (
  <div className="ss-auth-frame" style={{
    minHeight: "100%", position: "relative",
    display: "grid", gridTemplateRows: "auto 1fr auto",
    overflow: "hidden",
  }}>
    <div className="starfield" style={{ opacity: 0.4 }} />

    <header style={{ padding: "24px 32px", display: "flex", alignItems: "center", justifyContent: "space-between", position: "relative", zIndex: 1 }}>
      <button
        type="button"
        onClick={() => go("landing")}
        className="ss-mark"
        style={{ background: "transparent", border: 0, cursor: "pointer", color: "inherit" }}
      >
        <span className="ss-mark-glyph">★</span>
        <span>STARSTATS</span>
      </button>
      <span className="ss-eyebrow">{eyebrow}</span>
    </header>

    <main style={{
      display: "grid", placeItems: "center",
      padding: "20px 32px 60px",
      position: "relative", zIndex: 1,
    }}>
      {children}
    </main>

    <footer style={{
      padding: "20px 32px",
      color: "var(--fg-dim)", fontSize: 11, textAlign: "center",
      borderTop: "1px solid var(--border)",
    }}>
      Pairings, public profiles, and shares are governed by your account settings. Read the <a href="#" style={{ color: "var(--fg-muted)" }}>privacy notice</a>.
    </footer>
  </div>
);

/* ============================================================
 * Login
 * ========================================================== */
const Login = ({ go }) => (
  <AuthFrame go={go} eyebrow="Sign in">
    <div style={{ width: "100%", maxWidth: 400 }}>
      <Stack gap={24}>
        <div>
          <h1 style={{ margin: 0, fontSize: 28, fontWeight: 600, letterSpacing: "-0.02em" }}>
            Sign in
          </h1>
          <p style={{ margin: "8px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
            Welcome back. Use your Comm-Link and password.
          </p>
        </div>

        <Stack gap={14}>
          <Input label="Comm-Link" type="email" placeholder="you@example.com" autoComplete="email" defaultValue="t.saiyan@example.com" />
          <Input label="Password" type="password" autoComplete="current-password" defaultValue="••••••••••••" />
        </Stack>

        <Button kind="primary" onClick={() => go("totp")} iconRight={<I.arrow size={14} />}>Sign in</Button>

        <div style={{ display: "flex", alignItems: "center", gap: 12, color: "var(--fg-dim)", fontSize: 12 }}>
          <hr className="ss-rule" style={{ flex: 1 }} /> or <hr className="ss-rule" style={{ flex: 1 }} />
        </div>

        <Button kind="ghost" onClick={() => go("magicSent")} icon={<I.link size={14} />}>
          Sign in with one-time email link
        </Button>

        <hr className="ss-rule" />

        <div style={{ fontSize: 13, color: "var(--fg-muted)", display: "flex", flexDirection: "column", gap: 6 }}>
          <a href="#" style={{ color: "var(--fg-muted)" }}>Forgot your password?</a>
          <span>New here? <a href="#" onClick={(e) => { e.preventDefault(); go("signup"); }} style={{ color: "var(--accent)" }}>Create an account</a>.</span>
        </div>
      </Stack>
    </div>
  </AuthFrame>
);

/* ============================================================
 * Magic link sent
 * ========================================================== */
const MagicSent = ({ go }) => (
  <AuthFrame go={go} eyebrow="One-time link sent">
    <div style={{ width: "100%", maxWidth: 440 }}>
      <Stack gap={24}>
        <div style={{
          width: 48, height: 48, borderRadius: 12,
          background: "var(--accent-soft)", color: "var(--accent)",
          display: "grid", placeItems: "center",
          border: "1px solid color-mix(in oklab, var(--accent) 35%, transparent)",
        }}>
          <I.link size={20} />
        </div>
        <div>
          <h1 style={{ margin: 0, fontSize: 28, fontWeight: 600, letterSpacing: "-0.02em" }}>
            Check your Comm-Link.
          </h1>
          <p style={{ margin: "10px 0 0", color: "var(--fg-muted)", fontSize: 14, lineHeight: 1.55 }}>
            We sent a one-time sign-in link to{" "}
            <strong style={{ color: "var(--fg)" }} className="mono">t.saiyan@example.com</strong>.
            The link expires in 10 minutes and works exactly once.
          </p>
        </div>

        <div className="ss-alert" style={{ alignItems: "flex-start" }}>
          <I.bell size={14} />
          <span style={{ color: "var(--fg-muted)" }}>
            Didn&apos;t arrive? Check spam, or wait 30 seconds and{" "}
            <a href="#" style={{ color: "var(--accent)" }}>request another</a>. Old links are invalidated automatically.
          </span>
        </div>

        <Stack gap={8}>
          <Button kind="ghost" onClick={() => go("login")}>Back to sign in</Button>
        </Stack>
      </Stack>
    </div>
  </AuthFrame>
);

/* ============================================================
 * TOTP Verify (segmented 6-digit)
 * ========================================================== */
const TotpVerify = ({ go }) => {
  const [vals, setVals] = React.useState(["1", "2", "8", "", "", ""]);
  return (
    <AuthFrame go={go} eyebrow="Two-factor verification">
      <div style={{ width: "100%", maxWidth: 460 }}>
        <Stack gap={28}>
          <div>
            <h1 style={{ margin: 0, fontSize: 28, fontWeight: 600, letterSpacing: "-0.02em" }}>
              Authentication code.
            </h1>
            <p style={{ margin: "10px 0 0", color: "var(--fg-muted)", fontSize: 14, lineHeight: 1.55 }}>
              Open your authenticator app and type the 6-digit code for StarStats. Codes refresh every 30 seconds.
            </p>
          </div>

          <Stack gap={14}>
            <span className="ss-label-text">Code</span>
            <OTP values={vals} focusIdx={3} />
            <small style={{ color: "var(--fg-dim)", fontSize: 12 }}>
              Lost your authenticator? Type a recovery code (<span className="mono">XXXX-XXXX-XXXX-XXXX</span>) instead — each is single-use.
            </small>
          </Stack>

          <Row justify="space-between">
            <Button kind="ghost" onClick={() => go("login")}>Back</Button>
            <Button kind="primary" onClick={() => go("dashboard")} iconRight={<I.arrow size={14} />}>
              Verify &amp; sign in
            </Button>
          </Row>

          <div style={{
            padding: "12px 16px",
            background: "var(--bg-elev)",
            border: "1px solid var(--border)",
            borderRadius: "var(--r-sm)",
            color: "var(--fg-dim)", fontSize: 12, lineHeight: 1.5,
          }}>
            <strong style={{ color: "var(--fg-muted)" }}>Why this exists.</strong> Your interim sign-in token is single-use and expires in 5 minutes. Backing out invalidates it — that&apos;s by design.
          </div>
        </Stack>
      </div>
    </AuthFrame>
  );
};

/* ============================================================
 * Signup
 * ========================================================== */
const Signup = ({ go }) => (
  <AuthFrame go={go} eyebrow="Create account">
    <div style={{ width: "100%", maxWidth: 420 }}>
      <Stack gap={24}>
        <div>
          <h1 style={{ margin: 0, fontSize: 28, fontWeight: 600, letterSpacing: "-0.02em" }}>
            Create your account.
          </h1>
          <p style={{ margin: "8px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
            Comm-Link plus a password gets you a hangar. You can verify your RSI handle later.
          </p>
        </div>

        <Stack gap={14}>
          <Input label="Comm-Link" type="email" placeholder="you@example.com" />
          <Input label="Password" type="password" hint="At least 12 characters." />
          <Input label="Confirm password" type="password" />
        </Stack>

        <Button kind="primary" onClick={() => go("magicSent")} iconRight={<I.arrow size={14} />}>
          Create account
        </Button>

        <div style={{ fontSize: 12, color: "var(--fg-dim)", lineHeight: 1.5 }}>
          By continuing you agree to the <a href="#" style={{ color: "var(--fg-muted)" }}>terms</a> and <a href="#" style={{ color: "var(--fg-muted)" }}>privacy notice</a>. Nothing leaves your PC until you pair a device.
        </div>

        <hr className="ss-rule" />
        <div style={{ fontSize: 13, color: "var(--fg-muted)" }}>
          Already signed up? <a href="#" onClick={(e) => { e.preventDefault(); go("login"); }} style={{ color: "var(--accent)" }}>Sign in</a>.
        </div>
      </Stack>
    </div>
  </AuthFrame>
);

/* ============================================================
 * Download
 * ========================================================== */
const Download = ({ go }) => (
  <AuthFrame go={go} eyebrow="Download client">
    <div style={{ width: "100%", maxWidth: 520 }}>
      <Stack gap={24}>
        <div style={{
          width: 48, height: 48, borderRadius: 12,
          background: "var(--accent-soft)", color: "var(--accent)",
          display: "grid", placeItems: "center",
          border: "1px solid color-mix(in oklab, var(--accent) 35%, transparent)",
        }}>
          <I.download size={20} />
        </div>
        <div>
          <h1 style={{ margin: 0, fontSize: 28, fontWeight: 600, letterSpacing: "-0.02em" }}>
            Get the tray client.
          </h1>
          <p style={{ margin: "8px 0 0", color: "var(--fg-muted)", fontSize: 14, lineHeight: 1.55 }}>
            A small app that sits in your tray and reads what the game writes down. Nothing leaves your PC until you sign in and turn on sync.
          </p>
        </div>

        <Stack gap={10}>
          {[
            { os: "Windows", note: "10 / 11 · 64-bit", size: "14.2 MB" },
            { os: "macOS",   note: "13 Ventura+ · Universal", size: "16.8 MB" },
            { os: "Linux",   note: "AppImage / .deb · x86_64", size: "15.1 MB" },
          ].map(d => (
            <Row key={d.os} justify="space-between" style={{
              padding: "14px 16px", background: "var(--bg-elev)",
              border: "1px solid var(--border)", borderRadius: "var(--r-sm)",
              flexWrap: "nowrap",
            }}>
              <Stack gap={2}>
                <span style={{ fontWeight: 600, color: "var(--fg)" }}>{d.os}</span>
                <span style={{ fontSize: 12, color: "var(--fg-dim)" }}>{d.note}</span>
              </Stack>
              <Row gap={10} style={{ flexWrap: "nowrap" }}>
                <span className="mono" style={{ fontSize: 11, color: "var(--fg-dim)" }}>{d.size}</span>
                <Button kind="primary" icon={<I.download size={13} />}>Download</Button>
              </Row>
            </Row>
          ))}
        </Stack>

        <div style={{ fontSize: 12, color: "var(--fg-dim)", lineHeight: 1.6 }}>
          v0.4.2 · Pyro patch ready · <a href="https://github.com/starstats/starstats" target="_blank" rel="noreferrer" style={{ color: "var(--fg-muted)" }}>release notes</a>
        </div>

        <hr className="ss-rule" />
        <Row gap={10}>
          <Button kind="ghost" onClick={() => go("landing")}>Back to landing</Button>
          <Button kind="ghost" onClick={() => go("signup")} iconRight={<I.arrow size={13} />}>Create account</Button>
        </Row>
      </Stack>
    </div>
  </AuthFrame>
);

Object.assign(window, { Login, MagicSent, TotpVerify, AuthFrame, Signup, Download });
