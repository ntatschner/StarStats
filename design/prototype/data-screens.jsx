/**
 * StarStats — Data section: Metrics + Submissions.
 *
 * Metrics: what the desktop client has captured for THIS user.
 *   View tabs: Overview · Event types · Sessions · Raw stream
 *
 * Submissions: crowd-sourced "type discovery" — when the desktop client
 * encounters an unknown log line shape, it offers to submit the pattern
 * for community classification. Votes prioritize. Mods accept → next
 * parser update ships it to everyone.
 */

/* ============================================================
 * Shared bits
 * ========================================================== */

const StatusPill = ({ status }) => {
  const map = {
    pending:  { kind: "neutral", label: "Pending review" },
    review:   { kind: "warn",    label: "Under review" },
    accepted: { kind: "ok",      label: "Accepted" },
    shipped:  { kind: "ok",      label: "Shipped in 1.4.2" },
    rejected: { kind: "danger",  label: "Rejected" },
    flagged:  { kind: "danger",  label: "Flagged" },
  };
  const m = map[status] || { kind: "neutral", label: status };
  return <Badge kind={m.kind} dot>{m.label}</Badge>;
};

/* ============================================================
 * Metrics page
 * ========================================================== */

const METRICS_EVENT_TYPES = [
  { type: "quantum_target_selected", count: 412, pct: 17.1, last: "21:14:08", trend: "+18%" },
  { type: "vehicle_stowed",          count: 287, pct: 11.9, last: "21:13:42", trend: "+4%"  },
  { type: "actor_login",             count: 256, pct: 10.6, last: "20:31:14", trend: "+2%"  },
  { type: "mission_complete",        count: 142, pct:  5.9, last: "21:08:30", trend: "+11%" },
  { type: "actor_death",             count:  96, pct:  4.0, last: "20:59:11", trend: "−6%"  },
  { type: "vehicle_destruction",     count:  71, pct:  2.9, last: "19:58:21", trend: "+1%"  },
  { type: "mission_accepted",        count:  68, pct:  2.8, last: "19:42:09", trend: "+9%"  },
  { type: "quantum_jump_complete",   count:  62, pct:  2.6, last: "20:48:02", trend: "+3%"  },
  { type: "instance_join",           count:  44, pct:  1.8, last: "20:33:57", trend: "+0%"  },
  { type: "request_quit_lobby",      count:  31, pct:  1.3, last: "18:11:02", trend: "−2%"  },
];

const METRICS_SESSIONS = [
  { date: "Today",      start: "20:31", end: "21:14", dur: "0h 43m", ship: "600i Explorer",   client: "TheCodeSaiyan-PC",  events: 38 },
  { date: "Today",      start: "17:02", end: "19:18", dur: "2h 16m", ship: "Cutlass Black",   client: "TheCodeSaiyan-PC",  events: 142 },
  { date: "Yesterday",  start: "22:14", end: "00:48", dur: "2h 34m", ship: "Aurora MR",       client: "TheCodeSaiyan-LAP", events: 91 },
  { date: "Yesterday",  start: "19:30", end: "21:51", dur: "2h 21m", ship: "600i Explorer",   client: "TheCodeSaiyan-PC",  events: 118 },
  { date: "2 days ago", start: "20:11", end: "23:47", dur: "3h 36m", ship: "Carrack",         client: "TheCodeSaiyan-PC",  events: 204 },
  { date: "3 days ago", start: "21:02", end: "22:14", dur: "1h 12m", ship: "Aurora MR",       client: "TheCodeSaiyan-LAP", events: 47 },
  { date: "4 days ago", start: "18:44", end: "20:09", dur: "1h 25m", ship: "Cutlass Black",   client: "TheCodeSaiyan-PC",  events: 72 },
  { date: "5 days ago", start: "23:01", end: "23:58", dur: "0h 57m", ship: "C8 Pisces",       client: "TheCodeSaiyan-PC",  events: 31 },
];

const METRICS_RAW = [
  { ts: "21:14:08.214", type: "quantum_target_selected", payload: 'target="Crusader · Orison" · src="ARC-L1"' },
  { ts: "21:13:42.001", type: "vehicle_stowed",          payload: 'ship="600i Explorer" · location="Lorville"' },
  { ts: "21:08:30.918", type: "mission_complete",        payload: 'name="Investigate Comm Array" · system="Stanton-2" · reward=18500' },
  { ts: "20:59:11.452", type: "actor_death",             payload: 'cause="vehicle_destruction" · vehicle="Cutlass Black"' },
  { ts: "20:48:02.003", type: "quantum_jump_complete",   payload: 'src="ARC-L1" · dst="Hurston" · dur=42.1s' },
  { ts: "20:33:57.880", type: "instance_join",           payload: 'shard="us-west-2" · wave=8' },
  { ts: "20:31:14.012", type: "actor_login",             payload: 'character="TheCodeSaiyan"' },
];

const MetricsTab = ({ id, label, current, onClick, count }) => (
  <button
    type="button"
    onClick={() => onClick(id)}
    data-active={current === id ? "true" : undefined}
    style={{
      background: current === id ? "var(--bg-elev)" : "transparent",
      border: "1px solid",
      borderColor: current === id ? "var(--border-strong)" : "transparent",
      color: current === id ? "var(--fg)" : "var(--fg-muted)",
      padding: "8px 14px", borderRadius: "var(--r-pill)",
      cursor: "pointer", font: "inherit", fontSize: 13,
      display: "inline-flex", alignItems: "center", gap: 8,
    }}
  >
    <span>{label}</span>
    {count != null && (
      <span className="mono" style={{
        fontSize: 11, color: "var(--fg-dim)",
        padding: "2px 6px", background: "var(--bg)", borderRadius: 4,
      }}>{count}</span>
    )}
  </button>
);

const Metrics = ({ go }) => {
  const [view, setView] = React.useState("overview");

  return (
    <Stack gap={20}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Metrics · what the client has captured</div>
        <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>
          Your manifest
        </h1>
        <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
          Every event the desktop client has parsed, indexed by what kind it is and when.
          <span className="mono" style={{ color: "var(--fg)" }}> 2 412</span> events ·
          <span className="mono" style={{ color: "var(--fg)" }}> 184</span> sessions ·
          <span className="mono" style={{ color: "var(--fg)" }}> 47</span> distinct types.
        </p>
      </header>

      <Row gap={6} style={{ flexWrap: "wrap" }}>
        <MetricsTab id="overview"  label="Overview"   current={view} onClick={setView} />
        <MetricsTab id="types"     label="Event types" current={view} onClick={setView} count={47} />
        <MetricsTab id="sessions"  label="Sessions"   current={view} onClick={setView} count={184} />
        <MetricsTab id="raw"       label="Raw stream" current={view} onClick={setView} count="2 412" />
      </Row>

      {view === "overview"  && <MetricsOverview />}
      {view === "types"     && <MetricsTypes />}
      {view === "sessions"  && <MetricsSessions />}
      {view === "raw"       && <MetricsRaw />}
    </Stack>
  );
};

const MetricsOverview = () => (
  <Stack gap={20}>
    <Row gap={12} data-rsprow="nowrap" style={{ flexWrap: "nowrap" }}>
      {[
        { eyebrow: "Total events", value: "2 412", delta: "+184 this week", deltaKind: "ok" },
        { eyebrow: "Distinct types", value: "47",   delta: "3 new this month", deltaKind: "neutral" },
        { eyebrow: "Sessions",     value: "184",   delta: "Avg 1h 47m", deltaKind: "neutral" },
        { eyebrow: "Top type",     value: "QT_SEL", mono: true, delta: "412 captures", deltaKind: "neutral" },
      ].map((s, i) => (
        <div key={i} className="ss-card" style={{ flex: "1 1 200px", padding: "18px 20px" }}>
          <div className="ss-eyebrow">{s.eyebrow}</div>
          <div className="mono" style={{
            fontSize: 28, fontWeight: 500, letterSpacing: "-0.015em",
            margin: "8px 0 6px", color: "var(--fg)",
          }}>{s.value}</div>
          <div style={{
            color: s.deltaKind === "ok" ? "var(--ok)" : "var(--fg-dim)",
            fontSize: 12,
          }}>{s.delta}</div>
        </div>
      ))}
    </Row>

    <Row gap={20} data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", alignItems: "stretch" }}>
      <Card eyebrow="Top event types · last 30 days" title="What you do most">
        <Stack gap={10}>
          {METRICS_EVENT_TYPES.slice(0, 6).map((t, i) => (
            <div key={i} style={{ display: "grid", gridTemplateColumns: "1fr auto", gap: 6, alignItems: "baseline" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
                <span className="mono" style={{ fontSize: 13 }}>{t.type}</span>
                <span className="mono" style={{ fontSize: 12, color: "var(--fg-muted)" }}>{t.count}</span>
              </div>
              <div style={{ gridColumn: "1 / -1", height: 4, background: "var(--bg)", borderRadius: 2, overflow: "hidden" }}>
                <div style={{
                  height: "100%",
                  width: `${(t.count / METRICS_EVENT_TYPES[0].count) * 100}%`,
                  background: "var(--accent)",
                  opacity: 0.55 + (1 - i / 6) * 0.4,
                }} />
              </div>
            </div>
          ))}
        </Stack>
      </Card>

      <Card eyebrow="Recent sessions" title="Last week">
        <Stack gap={0}>
          {METRICS_SESSIONS.slice(0, 5).map((s, i) => (
            <div key={i} style={{
              display: "grid", gridTemplateColumns: "auto 1fr auto",
              gap: 14, alignItems: "center",
              padding: "10px 0",
              borderBottom: i < 4 ? "1px solid var(--border)" : "none",
            }}>
              <div className="mono" style={{ fontSize: 12, color: "var(--fg-muted)", minWidth: 90 }}>
                {s.date}
              </div>
              <div>
                <div style={{ fontSize: 13, color: "var(--fg)" }}>{s.ship}</div>
                <div className="mono" style={{ fontSize: 11, color: "var(--fg-dim)" }}>{s.start}–{s.end} · {s.dur}</div>
              </div>
              <div className="mono" style={{ fontSize: 12, color: "var(--accent)" }}>{s.events} ev</div>
            </div>
          ))}
        </Stack>
      </Card>
    </Row>
  </Stack>
);

const MetricsTypes = () => (
  <Card
    eyebrow="Event types · 47 distinct"
    title="What the client has classified"
    footer={<span style={{ fontSize: 12, color: "var(--fg-dim)" }}>Showing top 10. <a href="#" style={{ color: "var(--accent)" }}>Show all 47 →</a></span>}
  >
    <div className="ss-table-wrap">
      <table className="ss-table mono" style={{ fontSize: 13 }}>
        <thead>
          <tr>
            <th style={{ textAlign: "left" }}>Type</th>
            <th style={{ textAlign: "right" }}>Count</th>
            <th style={{ textAlign: "right" }}>% of total</th>
            <th style={{ textAlign: "right" }}>Last seen</th>
            <th style={{ textAlign: "right" }}>30d trend</th>
          </tr>
        </thead>
        <tbody>
          {METRICS_EVENT_TYPES.map((t, i) => (
            <tr key={i}>
              <td><span className="mono">{t.type}</span></td>
              <td style={{ textAlign: "right" }}>{t.count}</td>
              <td style={{ textAlign: "right", color: "var(--fg-muted)" }}>{t.pct}%</td>
              <td style={{ textAlign: "right", color: "var(--fg-muted)" }}>{t.last}</td>
              <td style={{ textAlign: "right", color: t.trend.startsWith("−") ? "var(--danger)" : t.trend === "+0%" ? "var(--fg-dim)" : "var(--ok)" }}>
                {t.trend}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  </Card>
);

const MetricsSessions = () => (
  <Card eyebrow="Sessions · 184 total" title="When you were flying">
    <div className="ss-table-wrap">
      <table className="ss-table" style={{ fontSize: 13 }}>
        <thead>
          <tr>
            <th style={{ textAlign: "left" }}>When</th>
            <th style={{ textAlign: "left" }}>Window</th>
            <th style={{ textAlign: "left" }}>Duration</th>
            <th style={{ textAlign: "left" }}>Primary ship</th>
            <th style={{ textAlign: "left" }}>Client</th>
            <th style={{ textAlign: "right" }}>Events</th>
          </tr>
        </thead>
        <tbody>
          {METRICS_SESSIONS.map((s, i) => (
            <tr key={i}>
              <td style={{ color: "var(--fg-muted)" }}>{s.date}</td>
              <td><span className="mono" style={{ fontSize: 12 }}>{s.start}–{s.end}</span></td>
              <td><span className="mono" style={{ fontSize: 12 }}>{s.dur}</span></td>
              <td>{s.ship}</td>
              <td><span className="mono" style={{ fontSize: 12, color: "var(--fg-muted)" }}>{s.client}</span></td>
              <td style={{ textAlign: "right" }}><span className="mono" style={{ color: "var(--accent)" }}>{s.events}</span></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  </Card>
);

const MetricsRaw = () => (
  <Card
    eyebrow="Raw event stream · last 7"
    title="Every line as parsed"
    footer={
      <Row gap={8}>
        <Button kind="ghost" icon={<I.download size={13} />}>Export full stream (JSON)</Button>
        <Button kind="ghost">Older →</Button>
      </Row>
    }
  >
    <Stack gap={0}>
      {METRICS_RAW.map((r, i) => (
        <div key={i} style={{
          display: "grid", gridTemplateColumns: "120px 200px 1fr",
          gap: 14, alignItems: "baseline",
          padding: "10px 0",
          borderBottom: i < METRICS_RAW.length - 1 ? "1px solid var(--border)" : "none",
          fontFamily: "var(--font-mono)", fontSize: 12,
        }}>
          <span style={{ color: "var(--fg-dim)" }}>{r.ts}</span>
          <span style={{ color: "var(--accent)" }}>{r.type}</span>
          <span style={{ color: "var(--fg-muted)" }}>{r.payload}</span>
        </div>
      ))}
    </Stack>
  </Card>
);

/* ============================================================
 * Submissions
 * ========================================================== */

const SUBMISSIONS = [
  {
    id: "SUB-2418",
    pattern: "<RemoteEventLogger> Forced Shutdown by *",
    label: "ship_forced_shutdown",
    description: "Triggered when an EMP or hostile action forces a vehicle into emergency shutdown.",
    submittedBy: "drift_pilot_47",
    submittedAt: "2 hours ago",
    occurrences: 1842,
    submitters: 213,
    votes: 487,
    voted: false,
    status: "review",
  },
  {
    id: "SUB-2417",
    pattern: "<Vehicle Soft Death> entity * health *",
    label: "vehicle_soft_death",
    description: "Vehicle hull breached but pilot survives — distinct from full destruction.",
    submittedBy: "TheCodeSaiyan",
    submittedAt: "8 hours ago",
    occurrences: 612,
    submitters: 84,
    votes: 312,
    voted: true,
    status: "review",
    mine: true,
  },
  {
    id: "SUB-2415",
    pattern: "<CIG-Net> Player rep change * faction *",
    label: "reputation_change",
    description: "Faction reputation delta. Useful for tracking wanted-level transitions and bounty work.",
    submittedBy: "stanton_runner",
    submittedAt: "1 day ago",
    occurrences: 4127,
    submitters: 461,
    votes: 891,
    voted: false,
    status: "accepted",
  },
  {
    id: "SUB-2412",
    pattern: "<MissionDelivery> ObjectContainerInteraction *",
    label: "cargo_handoff",
    description: "Box-on-box delivery interaction at mission endpoints.",
    submittedBy: "hauler_main",
    submittedAt: "2 days ago",
    occurrences: 743,
    submitters: 67,
    votes: 154,
    voted: false,
    status: "shipped",
  },
  {
    id: "SUB-2409",
    pattern: "<Spawn flow [Backend]> CCharacter::* spawn=*",
    label: "character_spawn",
    description: "Hab/medical spawn distinct from regular login. Spawn point and respawn timer included.",
    submittedBy: "med_gameplay",
    submittedAt: "3 days ago",
    occurrences: 2103,
    submitters: 298,
    votes: 612,
    voted: true,
    status: "accepted",
  },
  {
    id: "SUB-2401",
    pattern: "<Test scaffold> debug entity_remove *",
    label: "(suggested: debug_entity_remove)",
    description: "This is a development/debug log line, not part of normal gameplay.",
    submittedBy: "anon_4429",
    submittedAt: "4 days ago",
    occurrences: 18,
    submitters: 4,
    votes: 12,
    voted: false,
    status: "rejected",
    rejectionReason: "Internal debug log — not user-facing gameplay.",
  },
  {
    id: "SUB-2398",
    pattern: "<actor_state> stamina_drain duration=* cause=*",
    label: "stamina_event",
    description: "Stamina drain events from EVA, sprinting, combat actions.",
    submittedBy: "fps_main_hangar",
    submittedAt: "5 days ago",
    occurrences: 8204,
    submitters: 612,
    votes: 1204,
    voted: false,
    status: "review",
  },
];

const SUBMISSION_DETAIL = SUBMISSIONS[1];

const SubmissionRow = ({ s, onOpen, onVote, onFlag }) => {
  const [voted, setVoted] = React.useState(s.voted);
  const [voteCount, setVoteCount] = React.useState(s.votes);
  const handleVote = (e) => {
    e.stopPropagation();
    setVoted(v => !v);
    setVoteCount(c => voted ? c - 1 : c + 1);
  };
  return (
    <div
      onClick={() => onOpen(s.id)}
      className="ss-card"
      style={{
        display: "grid",
        gridTemplateColumns: "auto 1fr auto auto",
        gap: 18, alignItems: "center",
        padding: "16px 18px", cursor: "pointer",
      }}
    >
      {/* Vote column */}
      <button
        type="button"
        onClick={handleVote}
        title={voted ? "Remove vote" : "Vote to prioritize"}
        style={{
          display: "flex", flexDirection: "column", alignItems: "center", gap: 2,
          background: voted ? "var(--accent-soft)" : "var(--bg)",
          border: "1px solid",
          borderColor: voted ? "var(--accent)" : "var(--border)",
          color: voted ? "var(--accent)" : "var(--fg-muted)",
          borderRadius: "var(--r-md)",
          padding: "8px 12px", cursor: "pointer", minWidth: 56,
        }}
      >
        <I.arrowup size={14} />
        <span className="mono" style={{ fontSize: 13, fontWeight: 600 }}>{voteCount}</span>
      </button>

      {/* Body */}
      <div style={{ minWidth: 0 }}>
        <Row gap={8} style={{ marginBottom: 6 }}>
          <span className="mono" style={{ fontSize: 11, color: "var(--fg-dim)" }}>{s.id}</span>
          {s.mine && <Badge kind="accent" dot>Yours</Badge>}
          <StatusPill status={s.status} />
        </Row>
        <div className="mono" style={{
          fontSize: 13, color: "var(--accent)",
          marginBottom: 4, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
        }}>
          {s.label}
        </div>
        <div style={{ fontSize: 13, color: "var(--fg-muted)", marginBottom: 6 }}>
          {s.description}
        </div>
        <Row gap={14} style={{ fontSize: 11, color: "var(--fg-dim)" }}>
          <span>by <span className="mono" style={{ color: "var(--fg-muted)" }}>{s.submittedBy}</span></span>
          <span>{s.submittedAt}</span>
          <span><span className="mono" style={{ color: "var(--fg-muted)" }}>{s.occurrences.toLocaleString()}</span> occurrences</span>
          <span><span className="mono" style={{ color: "var(--fg-muted)" }}>{s.submitters}</span> submitters</span>
        </Row>
      </div>

      {/* Pattern preview */}
      <div className="mono" style={{
        fontSize: 11, color: "var(--fg-muted)",
        background: "var(--bg)", border: "1px solid var(--border)",
        borderRadius: "var(--r-sm)", padding: "6px 10px",
        maxWidth: 280, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
      }}>
        {s.pattern}
      </div>

      {/* Action overflow */}
      <button
        type="button"
        onClick={(e) => { e.stopPropagation(); onFlag(s.id); }}
        title="Flag as incorrect"
        style={{
          background: "transparent", border: "1px solid var(--border)",
          color: "var(--fg-dim)", borderRadius: "var(--r-sm)",
          padding: "6px 8px", cursor: "pointer",
        }}
      >
        <I.shield size={13} />
      </button>
    </div>
  );
};

const Submissions = ({ go }) => {
  const [filter, setFilter] = React.useState("all");
  const [detail, setDetail] = React.useState(null);

  const counts = {
    all:      SUBMISSIONS.length,
    review:   SUBMISSIONS.filter(s => s.status === "review").length,
    accepted: SUBMISSIONS.filter(s => s.status === "accepted" || s.status === "shipped").length,
    rejected: SUBMISSIONS.filter(s => s.status === "rejected").length,
    mine:     SUBMISSIONS.filter(s => s.mine).length,
  };

  const filtered = SUBMISSIONS.filter(s => {
    if (filter === "all") return true;
    if (filter === "review") return s.status === "review";
    if (filter === "accepted") return s.status === "accepted" || s.status === "shipped";
    if (filter === "rejected") return s.status === "rejected";
    if (filter === "mine") return s.mine;
    return true;
  });

  if (detail) {
    return <SubmissionDetail id={detail} onBack={() => setDetail(null)} />;
  }

  return (
    <Stack gap={20}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Submissions · type discovery</div>
        <Row gap={20} justify="space-between" align="flex-start" style={{ flexWrap: "wrap" }}>
          <div style={{ flex: "1 1 300px" }}>
            <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>
              Crowd-sourced signal
            </h1>
            <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14, maxWidth: 640 }}>
              When the desktop client sees a log line it doesn't recognise, it offers to send the pattern here.
              Vote to push useful ones up the queue. Accepted patterns ship in the next parser update — everyone gets the new metric.
            </p>
          </div>
          <Button kind="primary" icon={<I.plus size={13} />}>Submit a pattern</Button>
        </Row>
      </header>

      {/* Stat strip */}
      <Row gap={12} data-rsprow="nowrap" style={{ flexWrap: "nowrap" }}>
        {[
          { eyebrow: "Open submissions", value: "127", delta: "Across the network", deltaKind: "neutral" },
          { eyebrow: "Accepted this month", value: "12", delta: "Shipped in 1.4.x", deltaKind: "ok" },
          { eyebrow: "Your votes", value: "23", delta: "8 still open", deltaKind: "neutral" },
          { eyebrow: "Your submissions", value: "3", delta: "1 accepted, 2 in review", deltaKind: "neutral" },
        ].map((s, i) => (
          <div key={i} className="ss-card" style={{ flex: "1 1 200px", padding: "18px 20px" }}>
            <div className="ss-eyebrow">{s.eyebrow}</div>
            <div className="mono" style={{
              fontSize: 28, fontWeight: 500, letterSpacing: "-0.015em",
              margin: "8px 0 6px", color: "var(--fg)",
            }}>{s.value}</div>
            <div style={{
              color: s.deltaKind === "ok" ? "var(--ok)" : "var(--fg-dim)",
              fontSize: 12,
            }}>{s.delta}</div>
          </div>
        ))}
      </Row>

      {/* Filter tabs */}
      <Row gap={6} style={{ flexWrap: "wrap" }}>
        <MetricsTab id="all"      label="All"      current={filter} onClick={setFilter} count={counts.all} />
        <MetricsTab id="review"   label="In review" current={filter} onClick={setFilter} count={counts.review} />
        <MetricsTab id="accepted" label="Accepted" current={filter} onClick={setFilter} count={counts.accepted} />
        <MetricsTab id="rejected" label="Rejected" current={filter} onClick={setFilter} count={counts.rejected} />
        <MetricsTab id="mine"     label="Mine"     current={filter} onClick={setFilter} count={counts.mine} />
      </Row>

      {/* List */}
      <Stack gap={10}>
        {filtered.map(s => (
          <SubmissionRow
            key={s.id} s={s}
            onOpen={(id) => setDetail(id)}
            onVote={() => {}}
            onFlag={() => {}}
          />
        ))}
      </Stack>
    </Stack>
  );
};

const SubmissionDetail = ({ id, onBack }) => {
  const s = SUBMISSIONS.find(x => x.id === id) || SUBMISSION_DETAIL;
  const [voted, setVoted] = React.useState(s.voted);
  const [voteCount, setVoteCount] = React.useState(s.votes);
  const [flagged, setFlagged] = React.useState(false);

  return (
    <Stack gap={20}>
      <Row gap={10}>
        <button
          type="button" onClick={onBack}
          style={{
            background: "transparent", border: "1px solid var(--border)",
            color: "var(--fg-muted)", borderRadius: "var(--r-pill)",
            padding: "6px 14px 6px 10px", cursor: "pointer",
            font: "inherit", fontSize: 13,
            display: "inline-flex", alignItems: "center", gap: 6,
          }}
        >
          <I.arrow size={12} style={{ transform: "rotate(180deg)" }} /> All submissions
        </button>
      </Row>

      <header>
        <Row gap={8} style={{ marginBottom: 10 }}>
          <span className="mono" style={{ fontSize: 12, color: "var(--fg-dim)" }}>{s.id}</span>
          {s.mine && <Badge kind="accent" dot>Yours</Badge>}
          <StatusPill status={s.status} />
        </Row>
        <h1 className="mono" style={{ margin: 0, fontSize: 28, fontWeight: 500, letterSpacing: "-0.01em", color: "var(--accent)" }}>
          {s.label}
        </h1>
        <p style={{ margin: "10px 0 0", color: "var(--fg-muted)", fontSize: 14, maxWidth: 720 }}>
          {s.description}
        </p>
      </header>

      <Row gap={20} data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "2fr 1fr", alignItems: "stretch" }}>
        {/* Left: pattern + samples */}
        <Stack gap={20}>
          <Card eyebrow="Raw pattern" title="What the client matched">
            <div className="mono" style={{
              fontSize: 13, color: "var(--fg)",
              background: "var(--bg)", border: "1px solid var(--border)",
              borderRadius: "var(--r-md)", padding: "14px 16px",
              whiteSpace: "pre-wrap", wordBreak: "break-all",
            }}>
              {s.pattern}
            </div>
            <div style={{ marginTop: 10, fontSize: 12, color: "var(--fg-dim)" }}>
              <span className="mono">*</span> wildcards match any token.
              The proposed parser extracts <span className="mono">entity</span> and <span className="mono">health</span> as fields.
            </div>
          </Card>

          <Card eyebrow="Sample matches · last 24h" title="Real lines this caught">
            <Stack gap={0}>
              {[
                { ts: "21:07:14", line: "<Vehicle Soft Death> entity Cutlass_Black_4811 health 0.00" },
                { ts: "20:52:08", line: "<Vehicle Soft Death> entity 600i_Explorer_22a3 health 0.12" },
                { ts: "18:44:31", line: "<Vehicle Soft Death> entity Aurora_MR_881f health 0.04" },
                { ts: "16:11:02", line: "<Vehicle Soft Death> entity Cutlass_Black_aa17 health 0.00" },
              ].map((l, i, arr) => (
                <div key={i} style={{
                  display: "grid", gridTemplateColumns: "80px 1fr",
                  gap: 14, alignItems: "baseline",
                  padding: "10px 0",
                  borderBottom: i < arr.length - 1 ? "1px solid var(--border)" : "none",
                  fontFamily: "var(--font-mono)", fontSize: 12,
                }}>
                  <span style={{ color: "var(--fg-dim)" }}>{l.ts}</span>
                  <span style={{ color: "var(--fg-muted)" }}>{l.line}</span>
                </div>
              ))}
            </Stack>
          </Card>

          <Card eyebrow="Discussion · 14 comments" title="Community review">
            <Stack gap={14}>
              {[
                {
                  who: "stanton_runner", when: "1 hour ago",
                  body: "Confirmed — caught this on my Carrack last week when an EMP'd me at GrimHEX. Distinct enough from full destruction to warrant its own type.",
                  votes: 18,
                },
                {
                  who: "med_gameplay", when: "3 hours ago",
                  body: "Suggest renaming to ship_disable to match the umbrella we use internally for any forced-shutdown variant. Soft death is technically a subset.",
                  votes: 7,
                },
                {
                  who: "drift_pilot_47", when: "5 hours ago",
                  body: "+1 to acceptance. Would be useful for tracking insurance claims separately from full hull losses.",
                  votes: 4,
                },
              ].map((c, i, arr) => (
                <div key={i} style={{
                  paddingBottom: i < arr.length - 1 ? 14 : 0,
                  borderBottom: i < arr.length - 1 ? "1px solid var(--border)" : "none",
                }}>
                  <Row gap={10} style={{ marginBottom: 6 }}>
                    <span className="mono" style={{ fontSize: 12, color: "var(--fg)" }}>{c.who}</span>
                    <span style={{ fontSize: 11, color: "var(--fg-dim)" }}>{c.when}</span>
                    <span style={{ flex: 1 }} />
                    <span className="mono" style={{ fontSize: 11, color: "var(--fg-muted)" }}>+{c.votes}</span>
                  </Row>
                  <div style={{ fontSize: 13, color: "var(--fg-muted)", lineHeight: 1.55 }}>{c.body}</div>
                </div>
              ))}
            </Stack>
          </Card>
        </Stack>

        {/* Right: actions + meta */}
        <Stack gap={20}>
          <Card eyebrow="Your input" title="Help triage">
            <Stack gap={10}>
              <button
                type="button"
                onClick={() => { setVoted(v => !v); setVoteCount(c => voted ? c - 1 : c + 1); }}
                className="ss-btn"
                style={{
                  background: voted ? "var(--accent)" : "var(--bg-elev)",
                  color: voted ? "var(--accent-fg)" : "var(--fg)",
                  border: "1px solid",
                  borderColor: voted ? "var(--accent)" : "var(--border)",
                  borderRadius: "var(--r-md)",
                  padding: "12px 14px", cursor: "pointer",
                  font: "inherit", fontSize: 13, fontWeight: 500,
                  display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10,
                }}
              >
                <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
                  <I.arrowup size={13} />
                  {voted ? "Voted to prioritize" : "Vote to prioritize"}
                </span>
                <span className="mono" style={{ fontSize: 12 }}>{voteCount}</span>
              </button>

              <Button
                kind="ghost"
                icon={<I.shield size={13} />}
                onClick={() => setFlagged(f => !f)}
                style={flagged ? { borderColor: "var(--danger)", color: "var(--danger)" } : undefined}
              >
                {flagged ? "Flagged as incorrect" : "Flag as incorrect"}
              </Button>

              {s.mine && (
                <Button kind="ghost" icon={<I.trash size={13} />}>
                  Withdraw submission
                </Button>
              )}

              <div style={{
                fontSize: 11, color: "var(--fg-dim)",
                padding: "10px 12px", background: "var(--bg)",
                border: "1px solid var(--border)", borderRadius: "var(--r-sm)",
                lineHeight: 1.5,
              }}>
                Flag if the pattern catches the wrong thing or the proposed name is misleading.
                Three flags from verified handles auto-routes it to a moderator.
              </div>
            </Stack>
          </Card>

          <Card eyebrow="Submission" title="Details">
            <KV items={[
              ["Submitted by",   <span className="mono">{s.submittedBy}</span>],
              ["Submitted",      s.submittedAt],
              ["Occurrences",    <span className="mono">{s.occurrences.toLocaleString()}</span>],
              ["Submitters",     <span className="mono">{s.submitters}</span>],
              ["Total votes",    <span className="mono">{voteCount}</span>],
              ["Status",         <StatusPill status={s.status} />],
            ]} />
          </Card>

          <Card eyebrow="What happens next" title="Lifecycle">
            <Stack gap={12}>
              {[
                { phase: "Submitted",    state: "done",    note: "From 84 clients" },
                { phase: "Community vote", state: "active", note: `${voteCount} so far · 500 to advance` },
                { phase: "Mod review",   state: "pending", note: "Avg 4 days" },
                { phase: "Ships in update", state: "pending", note: "Next: 1.4.3" },
              ].map((p, i) => (
                <Row key={i} gap={12} align="center">
                  <span style={{
                    width: 10, height: 10, borderRadius: 999,
                    background: p.state === "done" ? "var(--ok)"
                              : p.state === "active" ? "var(--accent)"
                              : "var(--bg-elev)",
                    border: "1px solid",
                    borderColor: p.state === "active" ? "var(--accent)" : "var(--border)",
                    boxShadow: p.state === "active" ? "0 0 0 4px color-mix(in oklab, var(--accent) 20%, transparent)" : "none",
                    flexShrink: 0,
                  }} />
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 13, color: p.state === "pending" ? "var(--fg-muted)" : "var(--fg)" }}>{p.phase}</div>
                    <div style={{ fontSize: 11, color: "var(--fg-dim)" }}>{p.note}</div>
                  </div>
                </Row>
              ))}
            </Stack>
          </Card>
        </Stack>
      </Row>
    </Stack>
  );
};
