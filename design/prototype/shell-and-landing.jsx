/**
 * StarStats — app shell (top bar + left rail) and landing page.
 */

const HANDLE = "TheCodeSaiyan";

const NAV = [
  { id: "dashboard", label: "Dashboard", icon: I.chart, group: "main" },
  { id: "devices",   label: "Hangar",    icon: I.device, group: "main", note: "Paired clients" },
  { id: "orgs",      label: "Orgs",      icon: I.globe,  group: "main" },
  { id: "profile",   label: "Public profile", icon: I.user, group: "main" },
  { id: "metrics",     label: "Metrics",     icon: I.signal, group: "data", note: "What the client sees" },
  { id: "logs",        label: "My logs",     icon: I.chart,  group: "data", note: "Full archive" },
  { id: "submissions", label: "Submissions", icon: I.zap,    group: "data", note: "Type discovery" },
  { id: "settings",  label: "Settings",  icon: I.cog,    group: "account" },
  { id: "twofa",     label: "Two-factor",icon: I.shield, group: "account" },
  { id: "donate",    label: "Donate",    icon: I.heart,  group: "account", note: "Support StarStats" },
  { id: "components",label: "Components",icon: I.zap,    group: "system" },
];

const TopBar = ({ go, current }) => (
  <header className="ss-topbar">
    <button
      type="button"
      className="ss-drawer-toggle"
      aria-label="Open navigation"
      onClick={() => {
        const open = document.body.dataset.drawer === "open";
        document.body.dataset.drawer = open ? "closed" : "open";
      }}
      style={{ marginRight: 4 }}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
        <path d="M2 4h12M2 8h12M2 12h12" />
      </svg>
    </button>
    <button
      onClick={() => go("landing")}
      className="ss-mark"
      style={{ background: "transparent", border: 0, cursor: "pointer", color: "inherit" }}
    >
      <span className="ss-mark-glyph">★</span>
      <span>STARSTATS</span>
      <span className="ss-eyebrow" style={{ marginLeft: 6, color: "var(--fg-dim)", fontWeight: 500 }}>
        / {current === "landing" ? "marketing" : current}
      </span>
    </button>

    <div style={{ flex: 1 }} />

    <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
      <button
        type="button"
        onClick={() => go("dashboard")}
        style={{
          display: "flex", alignItems: "center", gap: 8,
          background: "var(--bg-elev)", border: "1px solid var(--border)",
          borderRadius: "var(--r-pill)", padding: "5px 5px 5px 12px",
          color: "var(--fg-muted)", fontSize: 12, cursor: "pointer",
        }}
      >
        <I.signal size={12} />
        <span className="mono" style={{ color: "var(--fg)" }}>2 clients online</span>
        <span style={{
          width: 22, height: 22, borderRadius: 999,
          background: "var(--accent)", color: "var(--accent-fg)",
          display: "grid", placeItems: "center", fontWeight: 700, fontSize: 11,
          fontFamily: "var(--font-mono)"
        }}>
          {HANDLE.slice(0, 2).toUpperCase()}
        </span>
      </button>
    </div>
  </header>
);

const DrawerScrim = () => (
  <div
    className="ss-drawer-scrim"
    onClick={() => { document.body.dataset.drawer = "closed"; }}
  />
);

const LeftRail = ({ go, current }) => {
  const main = NAV.filter(n => n.group === "main");
  const data = NAV.filter(n => n.group === "data");
  const account = NAV.filter(n => n.group === "account");
  const system = NAV.filter(n => n.group === "system");

  const Item = ({ n }) => (
    <button
      type="button"
      className="ss-rail-item"
      data-active={current === n.id ? "true" : undefined}
      onClick={() => { go(n.id); document.body.dataset.drawer = "closed"; }}
      style={{
        background: "transparent", border: "1px solid transparent",
        font: "inherit", color: "inherit", textAlign: "left",
        cursor: "pointer", width: "100%",
      }}
    >
      <n.icon size={15} />
      <span style={{ flex: 1 }}>{n.label}</span>
      {current === n.id && <span className="ss-rail-dot" />}
    </button>
  );

  return (
    <aside className="ss-rail">
      {main.map(n => <Item key={n.id} n={n} />)}
      <div className="ss-rail-section">Data</div>
      {data.map(n => <Item key={n.id} n={n} />)}
      <div className="ss-rail-section">Account</div>
      {account.map(n => <Item key={n.id} n={n} />)}
      <div className="ss-rail-section">System</div>
      {system.map(n => <Item key={n.id} n={n} />)}

      <div style={{ marginTop: "auto", padding: "16px 12px 0", borderTop: "1px solid var(--border)" }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>Signed in</div>
        <div className="mono" style={{ fontSize: 13, color: "var(--fg)" }}>{HANDLE}</div>
        <div style={{ fontSize: 11, color: "var(--fg-dim)", marginTop: 2 }}>RSI handle verified</div>
      </div>
    </aside>
  );
};

/* ============================================================
 * Hero word rotator — name swaps with feature callouts.
 * ========================================================== */
const HERO_WORDS = [
  { text: "StarStats.", kind: "brand" },
  { text: "Your manifest.", kind: "feature" },
  { text: "Your numbers.", kind: "feature" },
  { text: "Your timeline.", kind: "feature" },
];

const HeroRotator = () => {
  const [i, setI] = React.useState(0);
  const [phase, setPhase] = React.useState("in"); // in → out → next

  React.useEffect(() => {
    const id = setTimeout(() => setPhase("out"), 2400);
    return () => clearTimeout(id);
  }, [i]);

  React.useEffect(() => {
    if (phase === "out") {
      const id = setTimeout(() => {
        setI((i + 1) % HERO_WORDS.length);
        setPhase("in");
      }, 480);
      return () => clearTimeout(id);
    }
  }, [phase, i]);

  const w = HERO_WORDS[i];
  return (
    <span className="hero-rotator" style={{ minWidth: "11ch" }}>
      <span
        key={i + "-" + phase}
        className="hero-word"
        data-state={phase}
        data-kind={w.kind}
      >
        {w.text}
      </span>
    </span>
  );
};

/* ============================================================
 * Landing page
 * ========================================================== */
const Landing = ({ go }) => (
  <div className="ss-landing" style={{ minHeight: "100%", position: "relative" }}>
    <div className="starfield" />

    {/* Marketing top bar (lighter than the app shell) */}
    <header className="ss-marketing-nav" style={{
      display: "flex", alignItems: "center",
      padding: "20px 48px", borderBottom: "1px solid var(--border)",
      background: "var(--bg)", position: "relative", zIndex: 1,
    }}>
      <span className="ss-mark">
        <span className="ss-mark-glyph">★</span>
        <span>STARSTATS</span>
      </span>
      <span className="ss-eyebrow" style={{ marginLeft: 12, color: "var(--fg-dim)", fontWeight: 500 }}>
        Community telemetry · Unofficial
      </span>
      <div style={{ flex: 1 }} />
      <nav style={{ display: "flex", gap: 28, alignItems: "center", color: "var(--fg-muted)", fontSize: 13 }}>
        <a href="#" style={{ color: "inherit", textDecoration: "none" }}>Features</a>
        <a href="#" style={{ color: "inherit", textDecoration: "none" }}>Privacy</a>
        <a href="#" onClick={(e) => { e.preventDefault(); go("download"); }} style={{ color: "inherit", textDecoration: "none" }}>Download</a>
        <a
          href="https://github.com/starstats/starstats"
          target="_blank" rel="noreferrer noopener"
          style={{ color: "inherit", textDecoration: "none", display: "inline-flex", alignItems: "center", gap: 6 }}
          title="View source on GitHub"
        >
          <I.github size={14} /> GitHub
        </a>
        <Button kind="ghost" onClick={() => go("login")}>Sign in</Button>
        <Button kind="primary" onClick={() => go("signup")} iconRight={<I.arrow size={14} />}>Get started</Button>
      </nav>
    </header>

    <main style={{ position: "relative", zIndex: 1 }}>
      {/* Hero */}
      <section className="ss-hero" style={{
        padding: "120px 48px 80px",
        maxWidth: 1280, margin: "0 auto",
        display: "grid", gridTemplateColumns: "1.2fr 1fr", gap: 64,
      }}>
        <div>
          <div className="ss-eyebrow" style={{ marginBottom: 24, color: "var(--accent)" }}>
            <span style={{ display: "inline-block", width: 6, height: 6, borderRadius: 999, background: "var(--accent)", marginRight: 8, verticalAlign: "middle" }} />
            v0.4 · Pyro patch ready
          </div>
          <h1 style={{
            margin: 0, fontWeight: 600,
            fontSize: "clamp(40px, 6vw, 76px)",
            lineHeight: 1.02, letterSpacing: "-0.025em",
            color: "var(--fg)",
          }}>
            <span style={{ color: "var(--fg-muted)", fontSize: "0.6em", display: "block", marginBottom: 12, fontWeight: 400, letterSpacing: "-0.01em" }}>
              Track your Star Citizen play.
            </span>
            <HeroRotator />
          </h1>
          <p style={{
            color: "var(--fg-muted)",
            fontSize: 18, lineHeight: 1.55,
            maxWidth: 560, marginTop: 28,
          }}>
            A small app on your PC reads what the game already writes down — when you log in, where you fly, what you fly with. It stays on your machine until you sign in and turn on sync. Never your chat. Never anyone else.
          </p>
          <div className="ss-hero-buttons" style={{ display: "flex", gap: 12, marginTop: 36 }}>
            <Button kind="primary" onClick={() => go("signup")} iconRight={<I.arrow size={14} />}>
              Create account
            </Button>
            <Button kind="ghost" onClick={() => go("download")} icon={<I.download size={14} />}>
              Download tray client
            </Button>
          </div>
          <div style={{ marginTop: 32, display: "flex", gap: 24, alignItems: "center", color: "var(--fg-dim)", fontSize: 12 }}>
            <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <I.check size={12} /> Local-first
            </span>
            <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <I.check size={12} /> Open source client
            </span>
            <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <I.check size={12} /> Not affiliated with CIG
            </span>
          </div>
        </div>

        {/* Hero mockup — a stylised stat card preview */}
        <div className="ss-hero-mock" style={{ position: "relative", display: "flex", alignItems: "center", justifyContent: "center" }}>
          <div style={{
            position: "absolute", inset: -40,
            background: "radial-gradient(circle at 60% 40%, var(--accent-glow), transparent 60%)",
            filter: "blur(20px)", opacity: 0.5, pointerEvents: "none",
          }} />
          <div className="ss-card ss-card--elev" style={{
            position: "relative", width: "100%", maxWidth: 460,
            padding: "22px 24px",
            transform: "rotate(-1deg)",
          }}>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 18 }}>
              <div className="ss-eyebrow">Last 26 weeks</div>
              <Badge kind="accent" dot>Live</Badge>
            </div>
            <Heatmap weeks={22} seed={11} label="2 412 events tracked" />
            <hr className="ss-rule" style={{ margin: "18px 0" }} />
            <Row gap={20} style={{ flexWrap: "nowrap" }}>
              <Stack gap={2}>
                <div className="ss-eyebrow">Sessions</div>
                <div className="mono" style={{ fontSize: 22, color: "var(--fg)", letterSpacing: "-0.01em" }}>184</div>
              </Stack>
              <Stack gap={2}>
                <div className="ss-eyebrow">Hours</div>
                <div className="mono" style={{ fontSize: 22, color: "var(--fg)", letterSpacing: "-0.01em" }}>312.7</div>
              </Stack>
              <Stack gap={2}>
                <div className="ss-eyebrow">Top type</div>
                <div className="mono" style={{ fontSize: 13, color: "var(--accent)" }}>quantum_target</div>
              </Stack>
            </Row>
          </div>
        </div>
      </section>

      {/* Features grid */}
      <section className="ss-features" style={{ maxWidth: 1280, margin: "0 auto", padding: "0 48px 100px" }}>
        <div className="ss-eyebrow" style={{ marginBottom: 16 }}>What you get</div>
        <h2 style={{ margin: "0 0 48px", fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em", color: "var(--fg)", maxWidth: 720 }}>
          A telemetry tool, not a fan shrine. Built for players who want to read their own footprint.
        </h2>

        <div data-rspgrid="3" style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 16 }}>
          {[
            { icon: I.shield, title: "Stays on your PC by default", body: "Nothing leaves your machine until you sign in and turn on sync. You're in charge of when it talks to us." },
            { icon: I.chart,  title: "Just what you did in-game", body: "Logins, deaths, missions, jumps. Never your chat, never other players, never anything the game doesn't already show you." },
            { icon: I.link,   title: "Share what you want",    body: "Public profile, friends-only, org-only, or fully private. Prove an RSI handle is yours by pasting a code into your bio for one minute." },
            { icon: I.signal, title: "Tracks every PC you play on",      body: "Add as many machines as you like. Everything lands in one timeline, no double-counts, no gaps." },
            { icon: I.rocket, title: "Your numbers, your file",      body: "Per-day heatmap, top activities, full timeline. Download the whole thing as a single file whenever you want." },
            { icon: I.key,    title: "Locked-down sign-in",          body: "Magic link or password, two-factor with backup codes, per-device pairing. Handle verification stops anyone claiming yours." },
          ].map(f => (
            <div key={f.title} className="ss-card" style={{ padding: 22 }}>
              <div style={{
                width: 32, height: 32, borderRadius: 8,
                background: "var(--accent-soft)", color: "var(--accent)",
                display: "grid", placeItems: "center", marginBottom: 14,
                border: "1px solid color-mix(in oklab, var(--accent) 30%, transparent)",
              }}>
                <f.icon size={16} />
              </div>
              <h3 style={{ margin: "0 0 8px", fontSize: 15, fontWeight: 600, letterSpacing: "-0.01em" }}>{f.title}</h3>
              <p style={{ margin: 0, color: "var(--fg-muted)", fontSize: 13, lineHeight: 1.55 }}>{f.body}</p>
            </div>
          ))}
        </div>
      </section>

      {/* Footer */}
      <footer className="ss-footer" style={{
        borderTop: "1px solid var(--border)", padding: "32px 48px",
        display: "flex", justifyContent: "space-between", alignItems: "center",
        color: "var(--fg-dim)", fontSize: 12, gap: 24, flexWrap: "wrap",
      }}>
        <span>StarStats is a community tool. Not affiliated with Cloud Imperium Games or Roberts Space Industries.</span>
        <span style={{ display: "flex", gap: 18, alignItems: "center" }}>
          <a href="#" style={{ color: "inherit" }}>Privacy</a>
          <a href="#" style={{ color: "inherit" }}>Terms</a>
          <a
            href="https://github.com/starstats/starstats"
            target="_blank" rel="noreferrer noopener"
            style={{ color: "inherit", display: "inline-flex", alignItems: "center", gap: 6 }}
          >
            <I.github size={12} /> github.com/starstats/starstats
          </a>
        </span>
      </footer>
    </main>
  </div>
);

Object.assign(window, { TopBar, LeftRail, DrawerScrim, Landing, NAV, HANDLE });
