/**
 * StarStats — in-app screens (live behind the app shell):
 *   Dashboard, Devices (Hangar), Orgs, PublicProfile.
 */

const TIMELINE_DATA = [
  { time: "21:14:08", source: "TheCodeSaiyan-PC",  summary: "Quantum target selected: Crusader · Orison",         tag: "QT",    tagKind: "accent",  color: "var(--accent)" },
  { time: "21:13:42", source: "TheCodeSaiyan-PC",  summary: "Vehicle stowed: 600i Explorer at Lorville",          tag: "STOW",  tagKind: "neutral", color: "var(--info)" },
  { time: "21:08:30", source: "TheCodeSaiyan-PC",  summary: "Mission complete: Investigate Comm Array — Stanton-2",tag: "OK",    tagKind: "ok",      color: "var(--ok)" },
  { time: "20:59:11", source: "TheCodeSaiyan-PC",  summary: "Actor death — vehicle_destruction · Cutlass Black",  tag: "KIA",   tagKind: "danger",  color: "var(--danger)" },
  { time: "20:48:02", source: "TheCodeSaiyan-PC",  summary: "Quantum jump complete: ARC-L1 → Hurston",            tag: "QT",    tagKind: "accent",  color: "var(--accent)" },
  { time: "20:33:57", source: "TheCodeSaiyan-LAP", summary: "Join PU — instance us-west-2 · Wave 8",              tag: "JOIN",  tagKind: "ok",      color: "var(--ok)" },
  { time: "20:31:14", source: "TheCodeSaiyan-LAP", summary: "Legacy login — character TheCodeSaiyan",                                                color: "var(--ok)" },
  { time: "19:58:21", source: "TheCodeSaiyan-PC",  summary: "Vehicle destruction — Aurora MR (self) at Daymar",   tag: "DESTR", tagKind: "danger",  color: "var(--danger)" },
  { time: "19:42:09", source: "TheCodeSaiyan-PC",  summary: "Mission accepted: Recover stolen cargo · Stanton",                                      color: "var(--info)" },
  { time: "19:18:55", source: "TheCodeSaiyan-PC",  summary: "Quantum target selected: Hurston · Lorville",        tag: "QT",    tagKind: "accent",  color: "var(--accent)" },
];

const TOP_TYPES = [
  { type: "quantum_target_selected", count: 412 },
  { type: "vehicle_stowed",          count: 287 },
  { type: "mission_complete",        count: 142 },
  { type: "actor_death",             count: 96  },
  { type: "vehicle_destruction",     count: 71  },
];

/* ============================================================
 * Dashboard
 * ========================================================== */
const Dashboard = ({ go }) => (
  <Stack gap={20}>
    <header>
      <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Manifest · last 30 days</div>
      <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>
        Hi, <span className="mono" style={{ color: "var(--accent)", fontWeight: 500 }}>{HANDLE}</span>
      </h1>
      <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
        <span className="mono">2 412</span> events captured across <span className="mono">2</span> paired clients.
        Last sync <span className="mono">12s ago</span>.
      </p>
    </header>

    {/* Stat strip */}
    <Row gap={12} data-rsprow="nowrap" style={{ flexWrap: "nowrap" }}>
      {[
        { eyebrow: "Total events", value: "2 412", delta: "+184 this week", deltaKind: "ok" },
        { eyebrow: "Hours played", value: "312.7", delta: "+22.4h", deltaKind: "ok" },
        { eyebrow: "Sessions",     value: "184",   delta: "8 active streaks", deltaKind: "neutral" },
        { eyebrow: "Top system",   value: "Stanton", mono: false, delta: "61% of jumps", deltaKind: "neutral" },
      ].map((s, i) => (
        <div key={i} className="ss-card" style={{ flex: "1 1 200px", padding: "18px 20px" }}>
          <div className="ss-eyebrow">{s.eyebrow}</div>
          <div className={s.mono === false ? "" : "mono"} style={{
            fontSize: 28, fontWeight: 500, letterSpacing: "-0.015em",
            margin: "8px 0 6px", color: "var(--fg)",
          }}>
            {s.value}
          </div>
          <div style={{
            color: s.deltaKind === "ok" ? "var(--ok)" : "var(--fg-dim)",
            fontSize: 12,
          }}>
            {s.delta}
          </div>
        </div>
      ))}
    </Row>

    <Card eyebrow="Activity" title="Last 26 weeks">
      <Heatmap weeks={26} seed={9} label="2 412 events · most active day Wed Apr 22 (78 events)" />
    </Card>

    <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1.3fr", gap: 16 }}>
      <Card eyebrow="Distribution" title="Top event types"
        footer={<span style={{ color: "var(--fg-dim)", fontSize: 12 }}>Click any type to filter the timeline.</span>}>
        <TypeBars items={TOP_TYPES} total={TOP_TYPES.reduce((a, b) => a + b.count, 0)} />
      </Card>

      <Card
        eyebrow="Stream"
        title="Recent activity"
        footer={
          <Row justify="space-between">
            <Badge kind="accent">Filter: <span className="mono" style={{ marginLeft: 6 }}>type=quantum_target_selected</span></Badge>
            <Row gap={12}>
              <Button kind="link">← Newer</Button>
              <Button kind="link" iconRight={<I.arrow size={12} />}>Older</Button>
            </Row>
          </Row>
        }
      >
        <TimelineList events={TIMELINE_DATA.slice(0, 8)} />
      </Card>
    </div>
  </Stack>
);

/* ============================================================
 * Devices (Hangar)
 * ========================================================== */
const Devices = ({ go }) => (
  <Stack gap={20}>
    <header>
      <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Hangar · paired clients</div>
      <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>Pair a desktop client</h1>
      <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
        Run the StarStats tray app, click <em>Pair</em>, type the code below. Codes expire in 5 minutes and burn on first use.
      </p>
    </header>

    <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
      <Card eyebrow="Step 1" title="Generate a pairing code"
        footer={<Button kind="primary" iconRight={<I.arrow size={12} />}>Generate code</Button>}>
        <Stack gap={14}>
          <Input
            label="Device label"
            placeholder="Daisy's gaming PC"
            hint="Optional — helps you tell devices apart in the Hangar list."
          />
        </Stack>
      </Card>

      <Card eyebrow="Active code" title="Paste this into the tray app" >
        <Stack gap={16}>
          <div style={{
            background: "var(--bg-elev)", border: "1px dashed var(--accent)",
            borderRadius: "var(--r-md)", padding: "26px 24px", textAlign: "center",
          }}>
            <div className="mono ss-pair-code" style={{
              fontSize: 38, color: "var(--accent)", letterSpacing: "0.18em",
              fontWeight: 500,
            }}>
              7K2-94B
            </div>
            <div style={{ marginTop: 12, color: "var(--fg-dim)", fontSize: 12 }}>
              Expires in <span className="mono" style={{ color: "var(--fg-muted)" }}>~4m 23s</span>
            </div>
          </div>
          <div style={{ color: "var(--fg-dim)", fontSize: 12 }}>
            Each code is single-use. Generate a new one if it expires.
          </div>
        </Stack>
      </Card>
    </div>

    <Card eyebrow="Manifest" title="Paired devices (2)">
      <div className="ss-table-wrap">
      <table className="ss-table">
        <thead>
          <tr>
            <th>Label</th>
            <th>Paired</th>
            <th>Last seen</th>
            <th>Events</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {[
            { label: "TheCodeSaiyan-PC",  paired: "9d ago",  seen: "12s ago", events: "1 884", live: true },
            { label: "TheCodeSaiyan-LAP", paired: "23d ago", seen: "2h ago",  events: "528",   live: false },
          ].map(d => (
            <tr key={d.label}>
              <td>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <I.device size={14} />
                  <span className="mono">{d.label}</span>
                  {d.live && <Badge kind="ok" dot>Online</Badge>}
                </div>
              </td>
              <td style={{ color: "var(--fg-muted)" }}>{d.paired}</td>
              <td style={{ color: "var(--fg-muted)" }}>{d.seen}</td>
              <td className="mono ss-num">{d.events}</td>
              <td className="ss-num">
                <Button kind="link" style={{ color: "var(--danger)" }}>Revoke</Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      </div>
    </Card>
  </Stack>
);

/* ============================================================
 * Orgs
 * ========================================================== */
const Orgs = ({ go }) => (
  <Stack gap={20}>
    <header>
      <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Orgs</div>
      <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>Your orgs</h1>
      <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
        Orgs are loose groupings — a slug, a name, a list of members. Share your manifest with an org and every member can read it.
      </p>
    </header>

    <Row gap={12}>
      <Button kind="primary" icon={<I.plus size={12} />}>Create org</Button>
      <Button kind="ghost">Join by invite code</Button>
    </Row>

    <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 16 }}>
      {[
        { name: "Hardpoint Recovery", slug: "hardpoint", role: "Owner",  members: 14, you: true },
        { name: "Quiet Trade Co.",    slug: "qtco",      role: "Member", members: 38 },
        { name: "Outer Loop",         slug: "outer-loop",role: "Member", members: 7 },
      ].map(o => (
        <div key={o.slug} className="ss-card" style={{ padding: "20px 22px" }}>
          <Row justify="space-between" style={{ alignItems: "flex-start", marginBottom: 12 }}>
            <Stack gap={6}>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 600, letterSpacing: "-0.01em" }}>{o.name}</h3>
              <span className="mono" style={{ color: "var(--fg-dim)", fontSize: 12 }}>/orgs/{o.slug}</span>
            </Stack>
            {o.role === "Owner" ? <Badge kind="accent">Owner</Badge> : <Badge>Member</Badge>}
          </Row>
          <Row justify="space-between">
            <span style={{ color: "var(--fg-muted)", fontSize: 13 }}>
              <span className="mono">{o.members}</span> members
            </span>
            <Button kind="link" iconRight={<I.arrow size={12} />}>Open</Button>
          </Row>
        </div>
      ))}
    </div>

    <Card eyebrow="Empty state preview" title="When the manifest is clear">
      <div style={{
        textAlign: "center", padding: "40px 20px",
        color: "var(--fg-muted)", fontSize: 14,
      }}>
        <div style={{
          width: 48, height: 48, margin: "0 auto 16px",
          borderRadius: 12, background: "var(--bg-elev)",
          display: "grid", placeItems: "center",
          color: "var(--fg-dim)",
        }}>
          <I.globe size={20} />
        </div>
        <div style={{ fontSize: 16, color: "var(--fg)", marginBottom: 6 }}>Scope is clear.</div>
        <div>You&apos;re not a member of any orgs yet. Create one or paste an invite code above.</div>
      </div>
    </Card>
  </Stack>
);

/* ============================================================
 * Public profile (visitor view)
 * ========================================================== */
const PublicProfile = ({ go }) => (
  <Stack gap={20}>
    <header style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 24, flexWrap: "wrap" }}>
      <div>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Public profile</div>
        <h1 style={{ margin: 0, fontSize: 36, fontWeight: 600, letterSpacing: "-0.02em" }}>
          <span className="mono">{HANDLE}</span>
        </h1>
        <Row gap={10} style={{ marginTop: 10 }}>
          <Badge kind="accent" dot>Public profile</Badge>
          <Badge kind="ok">RSI verified</Badge>
          <Badge>Joined Jan 2025</Badge>
        </Row>
      </div>
      <Stack gap={6} style={{ alignItems: "flex-end" }}>
        <span className="ss-eyebrow">Profile URL</span>
        <Secret value={`starstats.app/u/${HANDLE}`} mask={false} />
      </Stack>
    </header>

    <Row gap={12} data-rsprow="nowrap" style={{ flexWrap: "nowrap" }}>
      {[
        { eyebrow: "Total events",  value: "2 412" },
        { eyebrow: "Hours played",  value: "312.7" },
        { eyebrow: "First sync",    value: "Jan 14" },
        { eyebrow: "Most-flown",    value: "600i", mono: false },
      ].map((s, i) => (
        <div key={i} className="ss-card" style={{ flex: "1 1 200px", padding: "18px 20px" }}>
          <div className="ss-eyebrow">{s.eyebrow}</div>
          <div className={s.mono === false ? "" : "mono"} style={{
            fontSize: 26, fontWeight: 500, letterSpacing: "-0.015em",
            margin: "8px 0 0", color: "var(--fg)",
          }}>{s.value}</div>
        </div>
      ))}
    </Row>

    <Card eyebrow="Activity" title="Last 26 weeks">
      <Heatmap weeks={26} seed={3} label="Public · 2 412 events" />
    </Card>

    <Card eyebrow="Distribution" title="Top event types">
      <TypeBars items={TOP_TYPES} total={TOP_TYPES.reduce((a, b) => a + b.count, 0)} />
    </Card>

    <div style={{
      padding: "14px 18px",
      background: "var(--bg-elev)",
      border: "1px solid var(--border)",
      borderRadius: "var(--r-sm)",
      color: "var(--fg-dim)", fontSize: 12, lineHeight: 1.5,
    }}>
      Public profiles show summary + top types only. The detailed timeline is only visible to handles or orgs the owner has explicitly shared with.
    </div>
  </Stack>
);

Object.assign(window, { Dashboard, Devices, Orgs, PublicProfile, TIMELINE_DATA, TOP_TYPES });
