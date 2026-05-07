/**
 * StarStats — main app: routes between screens, hosts theme,
 * exposes Tweaks (theme + type pairing + accent intensity).
 */

const SCREENS_WITH_SHELL = new Set([
  "dashboard", "devices", "orgs", "profile", "settings", "twofa", "donate", "components",
  "metrics", "submissions", "logs",
]);

const StarStatsApp = () => {
  const [t, setTweak] = useTweaks({
    theme: "stanton",
    typePairing: "geist",   // geist | inter | plex
    accent: 1.0,            // 0.65 .. 1.4 (multiplier on glow opacity)
    showRail: true,
  });
  const [screen, setScreen] = React.useState(() => {
    // Honor #screen=foo on first load so the responsive canvas
    // can deep-link individual frames.
    const m = /#screen=([\w]+)/.exec(location.hash || "");
    return m ? m[1] : "landing";
  });
  const [transKey, setTransKey] = React.useState(0);

  // Listen for postMessage from a parent canvas to switch screens in lockstep.
  React.useEffect(() => {
    const onMsg = (e) => {
      if (e.data && e.data.type === "__starstats_goto" && typeof e.data.screen === "string") {
        setScreen(e.data.screen);
      }
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  }, []);

  // Per-screen warp angle (degrees, 0=right, 90=down, 180=left, 270=up).
  // Only specified pages get a fixed direction; everything else gets a
  // randomized angle that's stable for the lifetime of that visit.
  const FIXED_ANGLE = {
    landing:    200,
    signup:     270,   // upward — new beginnings
    download:    90,   // downward — incoming file
    donate:     245,   // strong upward sweep
    twofa:      155,
  };
  const randomAngles = React.useRef({});
  const angleFor = (s) => {
    if (FIXED_ANGLE[s] != null) return FIXED_ANGLE[s];
    if (randomAngles.current[s] == null) {
      randomAngles.current[s] = Math.floor(Math.random() * 360);
    }
    return randomAngles.current[s];
  };
  const warpAngle = angleFor(screen);

  // Bump the transition key whenever screen changes so the wrapper re-mounts
  // and the entry animation replays.
  React.useEffect(() => { setTransKey((k) => k + 1); }, [screen]);

  // Apply theme + tweaks to the artboard.
  React.useEffect(() => {
    const el = document.getElementById("ss-root");
    if (!el) return;
    el.dataset.theme = t.theme;

    // Type pairing
    const pairs = {
      geist: { sans: "'Geist', ui-sans-serif, system-ui, sans-serif",
               mono: "'Geist Mono', ui-monospace, 'JetBrains Mono', Consolas, monospace" },
      inter: { sans: "'Inter', ui-sans-serif, system-ui, sans-serif",
               mono: "'JetBrains Mono', ui-monospace, Consolas, monospace" },
      plex:  { sans: "'IBM Plex Sans', ui-sans-serif, system-ui, sans-serif",
               mono: "'IBM Plex Mono', ui-monospace, Consolas, monospace" },
    };
    const p = pairs[t.typePairing] || pairs.geist;
    el.style.setProperty("--font-sans", p.sans);
    el.style.setProperty("--font-mono", p.mono);

    // Accent intensity — scales the glow alpha
    el.style.setProperty("--accent-glow-mul", String(t.accent));
  }, [t.theme, t.typePairing, t.accent]);

  const inAppShell = SCREENS_WITH_SHELL.has(screen);
  const screens = {
    landing:    <Landing go={setScreen} />,
    login:      <Login go={setScreen} />,
    magicSent:  <MagicSent go={setScreen} />,
    totp:       <TotpVerify go={setScreen} />,
    dashboard:  <Dashboard go={setScreen} />,
    devices:    <Devices go={setScreen} />,
    orgs:       <Orgs go={setScreen} />,
    profile:    <PublicProfile go={setScreen} />,
    settings:   <Settings go={setScreen} theme={t.theme} setTheme={(v) => setTweak("theme", v)} />,
    twofa:      <TwoFAWizard go={setScreen} />,
    donate:     <Donate go={setScreen} />,
    signup:     <Signup go={setScreen} />,
    download:   <Download go={setScreen} />,
    metrics:    <Metrics go={setScreen} />,
    submissions:<Submissions go={setScreen} />,
    logs:       <MyLogs go={setScreen} />,
    components: <Components go={setScreen} />,
  };

  return (
    <div
      id="ss-root"
      data-theme={t.theme}
      data-screen-label={`StarStats — ${screen}`}
      style={{
        height: "100%", minHeight: "100%",
        background: "var(--bg)", color: "var(--fg)",
        fontFamily: "var(--font-sans)",
        position: "relative",
      }}
    >
      {/* Animated background — sits behind everything */}
      <QuantumWarp angle={warpAngle} />
      <div className="ss-bg-pulse" />

      {inAppShell ? (
        <div className="ss-app" style={{ height: "100%", minHeight: "100vh", position: "relative", zIndex: 1 }}>
          <TopBar go={setScreen} current={screen} />
          <LeftRail go={setScreen} current={screen} />
          <DrawerScrim />
          <main className="ss-main">
            <div key={transKey} className="ss-screen-enter">
              {screens[screen]}
            </div>
          </main>
        </div>
      ) : (
        <div key={transKey} className="ss-screen-enter" style={{ position: "relative", zIndex: 1 }}>
          {screens[screen]}
        </div>
      )}

      <TweaksPanel title="Tweaks">
        <TweakSection title="Theme">
          <TweakSelect
            label="Color theme"
            value={t.theme}
            onChange={(v) => setTweak("theme", v)}
            options={[
              { value: "stanton", label: "Stanton — warm amber" },
              { value: "pyro",    label: "Pyro — molten coral" },
              { value: "terra",   label: "Terra — cool teal" },
              { value: "nyx",     label: "Nyx — light, deep violet" },
            ]}
          />
        </TweakSection>

        <TweakSection title="Type">
          <TweakRadio
            label="Pairing"
            value={t.typePairing}
            onChange={(v) => setTweak("typePairing", v)}
            options={[
              { value: "geist", label: "Geist" },
              { value: "inter", label: "Inter" },
              { value: "plex",  label: "Plex" },
            ]}
          />
        </TweakSection>

        <TweakSection title="Accent intensity">
          <TweakSlider
            label="Glow / focus halo"
            value={t.accent}
            onChange={(v) => setTweak("accent", v)}
            min={0.5} max={1.6} step={0.05}
          />
        </TweakSection>

        <TweakSection title="Layout">
          <TweakToggle
            label="Show left rail (in-app pages)"
            value={t.showRail}
            onChange={(v) => setTweak("showRail", v)}
          />
        </TweakSection>

        <TweakSection title="Jump to screen">
          <TweakSelect
            label="Screen"
            value={screen}
            onChange={(v) => setScreen(v)}
            options={[
              { value: "landing", label: "Landing" },
              { value: "login", label: "Auth — Login" },
              { value: "magicSent", label: "Auth — Magic link sent" },
              { value: "totp", label: "Auth — TOTP verify" },
              { value: "dashboard", label: "Dashboard" },
              { value: "devices", label: "Hangar (devices)" },
              { value: "orgs", label: "Orgs" },
              { value: "profile", label: "Public profile" },
              { value: "settings", label: "Settings" },
              { value: "twofa", label: "2FA wizard" },
              { value: "donate", label: "Donate / Support" },
              { value: "metrics", label: "Data — Metrics" },
              { value: "logs", label: "Data — My logs" },
              { value: "submissions", label: "Data — Submissions" },
              { value: "signup", label: "Auth — Sign up" },
              { value: "download", label: "Download client" },
              { value: "components", label: "Components" },
            ]}
          />
        </TweakSection>
      </TweaksPanel>
    </div>
  );
};

const root = ReactDOM.createRoot(document.getElementById("app-root"));
root.render(<StarStatsApp />);
