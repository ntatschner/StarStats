/**
 * StarStats — Settings, 2FA wizard (4-state), Components sheet.
 */

const THEMES = [
  { id: "stanton", name: "Stanton", subtitle: "Default · warm amber",
    swatch: ["#15131A", "#1A1820", "#2A2734", "#E8A23C", "#ECE7DD"] },
  { id: "pyro",    name: "Pyro",    subtitle: "Molten coral · aggressive",
    swatch: ["#1A1213", "#1F1517", "#321F22", "#F25C3F", "#F2E6E0"] },
  { id: "terra",   name: "Terra",   subtitle: "Cool teal · clinical",
    swatch: ["#0F161B", "#131C22", "#1F2C36", "#4FB8A1", "#E2EAEC"] },
  { id: "nyx",     name: "Nyx",     subtitle: "Light · deep violet",
    swatch: ["#ECE8E1", "#F7F4EE", "#FFFFFF", "#5B3FD9", "#1B1722"] },
];

const ThemeSwatch = ({ theme, active, onClick }) => (
  <button
    type="button"
    className="ss-theme-swatch"
    data-active={active ? "true" : undefined}
    onClick={onClick}
    style={{
      background: theme.swatch[1],
      color: theme.swatch[4],
      // ensure each card is "self-themed" — the colors come from the theme's tokens
      // by constructing them inline:
    }}
  >
    <Row justify="space-between" style={{ width: "100%", alignItems: "flex-start" }}>
      <Stack gap={2}>
        <span style={{ fontWeight: 600, fontSize: 14, letterSpacing: "-0.01em" }}>{theme.name}</span>
        <span style={{ fontSize: 11, opacity: 0.7 }}>{theme.subtitle}</span>
      </Stack>
      {active && (
        <span style={{
          width: 22, height: 22, borderRadius: 999,
          background: theme.swatch[3], color: theme.swatch[0],
          display: "grid", placeItems: "center", flexShrink: 0,
        }}>
          <I.check size={12} />
        </span>
      )}
    </Row>
    <div style={{ flex: 1 }} />
    {/* simulated chrome */}
    <div style={{
      background: theme.swatch[2],
      borderRadius: 5, padding: "6px 8px",
      display: "flex", alignItems: "center", justifyContent: "space-between", gap: 6,
    }}>
      <span style={{ fontSize: 10, fontFamily: "var(--font-mono)", opacity: 0.7 }}>★ STARSTATS</span>
      <span style={{
        height: 6, background: theme.swatch[3], borderRadius: 3,
        width: 36,
      }} />
    </div>
    <div className="ss-theme-swatch-bars">
      {[0.2, 0.4, 0.7, 1].map((o, i) => (
        <span key={i} style={{ background: theme.swatch[3], opacity: o }} />
      ))}
    </div>
  </button>
);

/* ============================================================
 * Settings
 * ========================================================== */
const Settings = ({ go, theme, setTheme }) => (
  <Stack gap={20}>
    <header>
      <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Account settings</div>
      <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>Preferences</h1>
      <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
        Account, sharing, security, and theme. Server actions are direct — no client framework involved.
      </p>
    </header>

    <Card eyebrow="Account" title="Account info">
      <KV items={[
        ["Comm-Link", <span><span className="mono">t.saiyan@example.com</span> <Badge kind="ok">Verified</Badge></span>],
        ["RSI handle", <span><span className="mono">{HANDLE}</span> <Badge kind="ok">Ownership proven</Badge></span>],
        ["Joined", <span className="mono">2025-01-14</span>],
        ["Supporter status", <span><Badge kind="neutral">Not donating</Badge> <a href="#" onClick={(e) => { e.preventDefault(); go("donate"); }} style={{ color: "var(--accent)", marginLeft: 8 }}>Become a supporter →</a></span>],
        ["Custom name plate", <span style={{ color: "var(--fg-dim)" }}>Locked · unlocks with any donation</span>],
        ["Data retention", <span><span className="mono">12 months</span> <span style={{ color: "var(--fg-dim)", marginLeft: 8 }}>· extends to 5 years with monthly support</span></span>],
      ]} />
    </Card>

    <Card eyebrow="Theme" title="Appearance"
      footer={<span style={{ color: "var(--fg-dim)", fontSize: 12 }}>
        Themes change accent + surface tints. Type, spacing, and component shapes are identical across all four.
      </span>}>
      <div data-rspgrid="4" style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 12 }}>
        {THEMES.map(t => (
          <ThemeSwatch
            key={t.id}
            theme={t}
            active={theme === t.id}
            onClick={() => setTheme(t.id)}
          />
        ))}
      </div>
    </Card>

    <Card eyebrow="Sharing" title="Public profile & access">
      <Stack gap={20}>
        <Row justify="space-between" style={{ alignItems: "flex-start" }}>
          <Stack gap={4} style={{ flex: 1 }}>
            <span style={{ fontWeight: 600, color: "var(--fg)" }}>Public profile</span>
            <span style={{ color: "var(--fg-muted)", fontSize: 13 }}>
              Anyone can view your summary and timeline at <span className="mono">/u/{HANDLE}</span>.
            </span>
          </Stack>
          <Toggle on={true} onChange={() => {}}>Public</Toggle>
        </Row>

        <hr className="ss-rule" />

        <Stack gap={12}>
          <span style={{ fontWeight: 600, color: "var(--fg)" }}>Shared with specific handles</span>
          <Stack gap={8}>
            {["DaisyAndromeda", "JulianFell", "Kestrel-Of-Crusader"].map(h => (
              <Row key={h} justify="space-between" style={{
                padding: "10px 14px", background: "var(--bg-elev)",
                border: "1px solid var(--border)", borderRadius: "var(--r-sm)",
              }}>
                <span className="mono">{h}</span>
                <Button kind="link" style={{ color: "var(--danger)" }}>Revoke</Button>
              </Row>
            ))}
          </Stack>
          <Row gap={8} style={{ flexWrap: "nowrap", alignItems: "flex-end" }}>
            <div style={{ flex: 1 }}>
              <Input label="Add by RSI handle" placeholder="EnterTheSquadron" />
            </div>
            <Button kind="primary">Grant access</Button>
          </Row>
        </Stack>

        <hr className="ss-rule" />

        <Stack gap={12}>
          <span style={{ fontWeight: 600, color: "var(--fg)" }}>Org shares</span>
          <Stack gap={8}>
            <Row justify="space-between" style={{
              padding: "10px 14px", background: "var(--bg-elev)",
              border: "1px solid var(--border)", borderRadius: "var(--r-sm)",
            }}>
              <span>
                <span className="mono">hardpoint</span>{" "}
                <span style={{ color: "var(--fg-dim)" }}>· Hardpoint Recovery</span>
              </span>
              <Button kind="link" style={{ color: "var(--danger)" }}>Revoke</Button>
            </Row>
          </Stack>
        </Stack>
      </Stack>
    </Card>

    <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
      <Card eyebrow="Security" title="Change password">
        <Stack gap={14}>
          <Input label="Current password" type="password" defaultValue="••••••••••••" />
          <Input label="New password" type="password" hint="At least 12 characters." />
          <Button kind="primary" style={{ alignSelf: "flex-start" }}>Update password</Button>
        </Stack>
      </Card>

      <Card eyebrow="Security" title="Two-factor authentication">
        <Stack gap={14}>
          <Row gap={10} style={{ alignItems: "flex-start" }}>
            <Badge kind="ok" dot>On</Badge>
            <span style={{ color: "var(--fg-muted)", fontSize: 13, lineHeight: 1.5 }}>
              Every sign-in asks for a 6-digit code from your authenticator app. Recovery codes were last regenerated <span className="mono">9d ago</span>.
            </span>
          </Row>
          <Row gap={10}>
            <Button kind="ghost" onClick={() => go("twofa")}>Manage 2FA</Button>
            <Button kind="ghost">Regenerate recovery codes</Button>
          </Row>
        </Stack>
      </Card>
    </div>

    <Card eyebrow="Account lifecycle" title="Change sign-in email">
      <Stack gap={14}>
        <p style={{ margin: 0, color: "var(--fg-muted)", fontSize: 13, lineHeight: 1.55 }}>
          We&apos;ll send a confirmation link to the new address; your sign-in email only changes after you click it. Your current address (<span className="mono">t.saiyan@example.com</span>) stays active until then.
        </p>
        <Row gap={8} style={{ flexWrap: "nowrap", alignItems: "flex-end" }}>
          <div style={{ flex: 1 }}>
            <Input label="New Comm-Link" type="email" placeholder="new@example.com" />
          </div>
          <Button kind="primary">Send confirmation link</Button>
        </Row>
      </Stack>
    </Card>

    <Card eyebrow="Danger zone" title="Delete account" danger
      footer={<Button kind="danger" icon={<I.trash size={12} />}>Delete my account</Button>}>
      <Stack gap={14}>
        <p style={{ margin: 0, color: "var(--fg-muted)", fontSize: 13, lineHeight: 1.55 }}>
          Deleting your account is permanent. Paired devices and active shares are removed. Your ingested events are pseudonymised — the row count is preserved so anyone you shared with keeps a coherent timeline, but the data is no longer linked to you.
        </p>
        <Input label="Type your handle to confirm" placeholder={HANDLE} />
      </Stack>
    </Card>
  </Stack>
);

/* ============================================================
 * 2FA wizard — 4 states side-by-side
 * ========================================================== */
const TwoFAWizard = ({ go }) => {
  const Step = ({ n, title, status, children }) => (
    <div className="ss-card" style={{ padding: 0, display: "flex", flexDirection: "column" }}>
      <div style={{
        display: "flex", alignItems: "center", justifyContent: "space-between",
        padding: "14px 18px", borderBottom: "1px solid var(--border)",
      }}>
        <Row gap={10}>
          <span style={{
            width: 22, height: 22, borderRadius: 999,
            background: status === "done" ? "var(--accent)" : "var(--bg-elev)",
            color: status === "done" ? "var(--accent-fg)" : "var(--fg-muted)",
            border: status === "active" ? "1px solid var(--accent)" : "1px solid var(--border)",
            display: "grid", placeItems: "center",
            fontSize: 11, fontFamily: "var(--font-mono)", fontWeight: 600,
          }}>
            {status === "done" ? <I.check size={11} /> : n}
          </span>
          <span style={{ fontWeight: 600, fontSize: 14, letterSpacing: "-0.01em" }}>{title}</span>
        </Row>
        <span className="ss-eyebrow" style={{
          color: status === "active" ? "var(--accent)" : status === "done" ? "var(--ok)" : "var(--fg-dim)"
        }}>
          {status === "done" ? "Complete" : status === "active" ? "In flight" : "Pending"}
        </span>
      </div>
      <div style={{ padding: 18, flex: 1 }}>{children}</div>
    </div>
  );

  return (
    <Stack gap={20}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Two-factor authentication</div>
        <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>Setup wizard</h1>
        <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
          The full enable-flow as it appears at <span className="mono">/settings/2fa</span>. Four states shown side-by-side.
        </p>
      </header>

      <div data-rspgrid="4" style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 14 }}>
        <Step n={1} title="2FA off" status="done">
          <Stack gap={12}>
            <Badge kind="warn">Off</Badge>
            <p style={{ margin: 0, color: "var(--fg-muted)", fontSize: 13, lineHeight: 1.5 }}>
              Account is protected by your password only. Adding a second factor stops anyone with a stolen password from signing in.
            </p>
            <Button kind="primary">Enable 2FA</Button>
          </Stack>
        </Step>

        <Step n={2} title="Scan and confirm" status="active">
          <Stack gap={12}>
            <span style={{ color: "var(--fg-muted)", fontSize: 12, lineHeight: 1.5 }}>
              Scan in your authenticator app, or paste the secret manually.
            </span>
            <div style={{
              background: "var(--bg-elev)", border: "1px solid var(--border)",
              borderRadius: 8, padding: 14,
              display: "grid", gridTemplateColumns: "76px 1fr", gap: 12, alignItems: "center",
            }}>
              {/* Stylised QR — abstract pattern, not a real code */}
              <svg viewBox="0 0 7 7" width="76" height="76" style={{ background: "var(--fg)", padding: 4, borderRadius: 4 }}>
                {[
                  [0,0],[1,0],[2,0],[4,0],[6,0],
                  [0,1],[2,1],[3,1],[5,1],[6,1],
                  [0,2],[1,2],[2,2],[4,2],[6,2],
                  [3,3],[5,3],
                  [0,4],[2,4],[3,4],[4,4],[6,4],
                  [1,5],[5,5],[6,5],
                  [0,6],[2,6],[3,6],[5,6],[6,6],
                ].map(([x,y]) => <rect key={`${x}-${y}`} x={x} y={y} width="1" height="1" fill="var(--bg)" />)}
              </svg>
              <Stack gap={4}>
                <span className="ss-eyebrow">Secret</span>
                <span className="mono" style={{ fontSize: 11, color: "var(--fg)", wordBreak: "break-all", lineHeight: 1.4 }}>
                  JBSW Y3DP EHPK 3PXP JBSW Y3DP
                </span>
                <span style={{ fontSize: 10, color: "var(--fg-dim)" }}>SHA-1 · 6 digits · 30s</span>
              </Stack>
            </div>
            <span className="ss-label-text">Type the 6-digit code</span>
            <div className="ss-otp" style={{ gap: 4 }}>
              {[..."183"].map((v, i) => (
                <input key={i} className="ss-otp-cell mono" defaultValue={v} maxLength={1} style={{ width: 28, height: 36, fontSize: 16 }} />
              ))}
              <span className="ss-otp-sep" />
              {[..."__"].map((v, i) => (
                <input key={i+3} className="ss-otp-cell mono" maxLength={1} style={{ width: 28, height: 36, fontSize: 16 }} data-focused={i === 0 ? "true" : undefined} />
              ))}
              <input className="ss-otp-cell mono" maxLength={1} style={{ width: 28, height: 36, fontSize: 16 }} />
            </div>
          </Stack>
        </Step>

        <Step n={3} title="Save recovery codes" status="pending">
          <Stack gap={12}>
            <span style={{ color: "var(--fg-muted)", fontSize: 12, lineHeight: 1.5 }}>
              Ten single-use codes. We can&apos;t show them again.
            </span>
            <pre className="mono" style={{
              margin: 0, background: "var(--bg-elev)",
              border: "1px solid var(--border)", borderRadius: 6,
              padding: 12, fontSize: 11, lineHeight: 1.7,
              color: "var(--fg)",
            }}>
{`A8K2-Q7VB-MN4X-P9LT
RT3Z-9WCN-VK7Q-8JBE
H6M4-2KX9-RPQT-N7BV
JF5L-8WMQ-RB2X-9KCN
2YT9-MN4K-7VBR-X8LP
BR3X-K9MN-T7VQ-P2YL
N7XK-9PQB-RM4T-V8JC
8KR3-LM2N-X9PB-Q7VT
P9TQ-R2YK-3MNV-X8BL
RM4X-K9TQ-7BNV-P2YL`}
            </pre>
            <Button kind="primary">I&apos;ve saved them</Button>
          </Stack>
        </Step>

        <Step n={4} title="2FA on" status="pending">
          <Stack gap={12}>
            <Badge kind="ok" dot>Enabled</Badge>
            <p style={{ margin: 0, color: "var(--fg-muted)", fontSize: 13, lineHeight: 1.5 }}>
              Sign-ins from this account require a 6-digit code from your authenticator app, or a one-shot recovery code.
            </p>
            <Stack gap={8}>
              <Button kind="ghost">Regenerate recovery codes</Button>
              <Button kind="danger">Turn off 2FA</Button>
            </Stack>
          </Stack>
        </Step>
      </div>
    </Stack>
  );
};

/* ============================================================
 * Components sheet
 * ========================================================== */
const Components = ({ go }) => {
  const Section = ({ title, eyebrow, children }) => (
    <section>
      <div style={{ marginBottom: 16 }}>
        <div className="ss-eyebrow" style={{ marginBottom: 6 }}>{eyebrow}</div>
        <h2 style={{ margin: 0, fontSize: 18, fontWeight: 600, letterSpacing: "-0.01em" }}>{title}</h2>
      </div>
      <div className="ss-card" style={{ padding: 24 }}>{children}</div>
    </section>
  );

  return (
    <Stack gap={28}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>System</div>
        <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>Components</h1>
        <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
          Every primitive in the StarStats kit. Tokens come from <span className="mono">tokens.css</span>; behaviour from <span className="mono">primitives.jsx</span>.
        </p>
      </header>

      <Section eyebrow="01" title="Buttons">
        <Stack gap={20}>
          <Row gap={12}>
            <Button kind="primary">Primary</Button>
            <Button kind="primary" iconRight={<I.arrow size={12} />}>Continue</Button>
            <Button kind="primary" icon={<I.plus size={12} />}>Add</Button>
            <Button kind="ghost">Ghost</Button>
            <Button kind="ghost" icon={<I.download size={12} />}>Download</Button>
            <Button kind="danger" icon={<I.trash size={12} />}>Delete</Button>
            <Button kind="link">Link button</Button>
          </Row>
          <hr className="ss-rule" />
          <Stack gap={8}>
            <span className="ss-eyebrow">States · primary</span>
            <Row gap={12}>
              <Button kind="primary">Default</Button>
              <Button kind="primary" style={{ boxShadow: "0 0 0 1px var(--accent), 0 0 14px -2px var(--accent-glow)" }}>Hover</Button>
              <Button kind="primary" style={{ transform: "scale(0.98)" }}>Active</Button>
              <Button kind="primary" style={{ outline: "2px solid var(--accent)", outlineOffset: 2 }}>Focus</Button>
              <Button kind="primary" disabled style={{ opacity: 0.5, cursor: "not-allowed" }}>Disabled</Button>
            </Row>
          </Stack>
        </Stack>
      </Section>

      <Section eyebrow="02" title="Inputs">
        <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 24 }}>
          <Stack gap={14}>
            <Input label="Comm-Link" placeholder="you@example.com" />
            <Input label="Password" type="password" defaultValue="••••••••" />
            <Input label="With suffix" defaultValue="TheCodeSaiyan" suffix="@RSI" />
            <Input label="With hint" placeholder="Optional" hint="Plain text. Trailing whitespace is trimmed." />
          </Stack>
          <Stack gap={14}>
            <span className="ss-label-text">Authentication code</span>
            <OTP values={["1","8","3","","",""]} focusIdx={3} />
            <span className="ss-label-text" style={{ marginTop: 8 }}>Textarea</span>
            <textarea className="ss-input" rows={4} defaultValue="Cargo manifest, Stanton run · Crusader → Lorville. ETA 14m." style={{ fontFamily: "var(--font-mono)", fontSize: 12 }} />
          </Stack>
        </div>
      </Section>

      <Section eyebrow="03" title="Badges & alerts">
        <Stack gap={20}>
          <Row gap={10}>
            <Badge>Neutral</Badge>
            <Badge kind="ok" dot>OK</Badge>
            <Badge kind="warn" dot>Warning</Badge>
            <Badge kind="danger" dot>Danger</Badge>
            <Badge kind="accent" dot>Accent</Badge>
          </Row>
          <hr className="ss-rule" />
          <Stack gap={10}>
            <div className="ss-alert ss-alert--ok">
              <I.check size={14} />
              <span>Password updated.</span>
            </div>
            <div className="ss-alert ss-alert--warn">
              <I.bell size={14} />
              <span>Verification email sent. Check your inbox.</span>
            </div>
            <div className="ss-alert ss-alert--danger">
              <I.x size={14} />
              <span>Code expired. Request a new one.</span>
            </div>
          </Stack>
        </Stack>
      </Section>

      <Section eyebrow="04" title="Cards">
        <div data-rspgrid="3" style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 14 }}>
          <Card eyebrow="Headline only" title="Pyro patch is ready">
            <span style={{ color: "var(--fg-muted)", fontSize: 13 }}>The 0.4 release lands tray-side telemetry for the Pyro system. Pair the new tray version on your Hangar page.</span>
          </Card>
          <Card title="With body & actions"
            footer={<Row gap={8}><Button kind="primary">Pair</Button><Button kind="ghost">Skip</Button></Row>}>
            <span style={{ color: "var(--fg-muted)", fontSize: 13 }}>You have one new device pending pairing. Codes expire in 5 minutes.</span>
          </Card>
          <Card eyebrow="Danger" title="Burn this account" danger
            footer={<Button kind="danger" icon={<I.trash size={12} />}>Delete</Button>}>
            <span style={{ color: "var(--fg-muted)", fontSize: 13 }}>Removes the account row, paired devices, and active shares.</span>
          </Card>
        </div>
      </Section>

      <Section eyebrow="05" title="Toggles & KV list">
        <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 24 }}>
          <Stack gap={14}>
            <Row justify="space-between"><span>Public profile</span><Toggle on={true} onChange={() => {}} /></Row>
            <Row justify="space-between"><span>Email notifications</span><Toggle on={false} onChange={() => {}} /></Row>
            <Row justify="space-between"><span>Beta channel</span><Toggle on={true} onChange={() => {}} /></Row>
            <Row justify="space-between"><span>Reduce motion</span><Toggle on={false} onChange={() => {}} /></Row>
          </Stack>
          <KV items={[
            ["Comm-Link", <span className="mono">t.saiyan@example.com</span>],
            ["RSI handle", <span className="mono">{HANDLE}</span>],
            ["User ID", <span className="mono" style={{ fontSize: 12, color: "var(--fg-muted)" }}>usr_01HQ4T9XV3K8M2N7</span>],
            ["Joined", <span className="mono">2025-01-14</span>],
          ]} />
        </div>
      </Section>

      <Section eyebrow="06" title="Code & secret blocks">
        <Stack gap={14}>
          <span className="ss-label-text">Default · masked</span>
          <Secret value="JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXPJBSWY3DP" mask={true} />
          <span className="ss-label-text">Revealed</span>
          <Secret value="JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXPJBSWY3DP" mask={false} />
          <span className="ss-label-text">Public · profile URL</span>
          <Secret value="https://starstats.app/u/TheCodeSaiyan" mask={false} />
        </Stack>
      </Section>

      <Section eyebrow="07" title="Tables">
        <table className="ss-table">
          <thead>
            <tr>
              <th>Type</th>
              <th>Source</th>
              <th>Last seen</th>
              <th className="ss-num">Count</th>
              <th className="ss-num">% of total</th>
            </tr>
          </thead>
          <tbody>
            {TOP_TYPES.map((t, i) => (
              <tr key={t.type}>
                <td className="mono">{t.type}</td>
                <td style={{ color: "var(--fg-muted)" }}>tray-windows</td>
                <td className="mono" style={{ color: "var(--fg-muted)" }}>{["12s", "4m", "1h", "3h", "9h"][i]} ago</td>
                <td className="mono ss-num">{t.count.toLocaleString()}</td>
                <td className="mono ss-num">{((t.count / TOP_TYPES.reduce((a, b) => a + b.count, 0)) * 100).toFixed(1)}%</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Section>

      <Section eyebrow="08" title="Empty states">
        <div data-rspgrid="3" style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 14 }}>
          {[
            { title: "Scope is clear.", body: "You're not a member of any orgs yet.", icon: I.globe },
            { title: "No contacts logged yet.", body: "Pair a device and play to see events here.", icon: I.signal },
            { title: "Manifest empty.", body: "Start by pairing a desktop client on your Hangar page.", icon: I.device },
          ].map((e, i) => (
            <div key={i} style={{
              padding: "32px 16px", textAlign: "center",
              border: "1px dashed var(--border-strong)", borderRadius: "var(--r-md)",
            }}>
              <div style={{
                width: 40, height: 40, margin: "0 auto 12px",
                borderRadius: 10, background: "var(--bg-elev)",
                display: "grid", placeItems: "center", color: "var(--fg-dim)",
              }}>
                <e.icon size={18} />
              </div>
              <div style={{ fontSize: 14, color: "var(--fg)", marginBottom: 4 }}>{e.title}</div>
              <div style={{ color: "var(--fg-muted)", fontSize: 12 }}>{e.body}</div>
            </div>
          ))}
        </div>
      </Section>
    </Stack>
  );
};

Object.assign(window, { Settings, TwoFAWizard, Components, THEMES, ThemeSwatch });
