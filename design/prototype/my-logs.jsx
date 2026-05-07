/**
 * StarStats — My Logs page
 *
 * The complete archive of every log batch the desktop client has sent up.
 * Different from Metrics (which is parsed/aggregated event data) — this is
 * the raw transmission record: every batch, every line, with deep metrics
 * about ingestion health, parser coverage, and storage.
 */

const LOG_BATCHES = [
  { id: "BATCH-9412", ts: "21:14:08", client: "TheCodeSaiyan-PC",  lines: 248,  parsed: 244, unknown: 4,  size: "47.2 KB", status: "ingested", session: "S-184" },
  { id: "BATCH-9411", ts: "21:13:42", client: "TheCodeSaiyan-PC",  lines: 12,   parsed: 12,  unknown: 0,  size: "2.1 KB",  status: "ingested", session: "S-184" },
  { id: "BATCH-9410", ts: "21:08:30", client: "TheCodeSaiyan-PC",  lines: 89,   parsed: 87,  unknown: 2,  size: "18.4 KB", status: "ingested", session: "S-184" },
  { id: "BATCH-9408", ts: "20:59:11", client: "TheCodeSaiyan-PC",  lines: 142,  parsed: 138, unknown: 4,  size: "29.1 KB", status: "ingested", session: "S-184" },
  { id: "BATCH-9402", ts: "20:31:14", client: "TheCodeSaiyan-LAP", lines: 38,   parsed: 38,  unknown: 0,  size: "7.2 KB",  status: "ingested", session: "S-184" },
  { id: "BATCH-9398", ts: "19:42:09", client: "TheCodeSaiyan-PC",  lines: 412,  parsed: 401, unknown: 11, size: "84.7 KB", status: "ingested", session: "S-183" },
  { id: "BATCH-9391", ts: "18:11:02", client: "TheCodeSaiyan-PC",  lines: 73,   parsed: 73,  unknown: 0,  size: "14.1 KB", status: "ingested", session: "S-183" },
  { id: "BATCH-9387", ts: "Yest 23:48", client: "TheCodeSaiyan-PC", lines: 891, parsed: 870, unknown: 21, size: "172 KB",  status: "ingested", session: "S-182" },
  { id: "BATCH-9385", ts: "Yest 22:14", client: "TheCodeSaiyan-LAP",lines: 14,  parsed: 0,   unknown: 0,  size: "3.0 KB",  status: "failed",   session: "S-181", error: "Sequence gap detected · awaiting retry" },
  { id: "BATCH-9384", ts: "Yest 21:51", client: "TheCodeSaiyan-PC", lines: 217, parsed: 213, unknown: 4,  size: "44.8 KB", status: "ingested", session: "S-181" },
  { id: "BATCH-9381", ts: "Yest 19:30", client: "TheCodeSaiyan-PC", lines: 98,  parsed: 96,  unknown: 2,  size: "20.1 KB", status: "ingested", session: "S-181" },
  { id: "BATCH-9374", ts: "2d 23:47",   client: "TheCodeSaiyan-PC", lines: 1240,parsed: 1218,unknown: 22, size: "248 KB",  status: "ingested", session: "S-180" },
  { id: "BATCH-9362", ts: "2d 20:11",   client: "TheCodeSaiyan-PC", lines: 84,  parsed: 84,  unknown: 0,  size: "16.4 KB", status: "ingested", session: "S-180" },
  { id: "BATCH-9359", ts: "3d 22:14",   client: "TheCodeSaiyan-LAP",lines: 312, parsed: 308, unknown: 4,  size: "61.2 KB", status: "ingested", session: "S-179" },
  { id: "BATCH-9351", ts: "3d 21:02",   client: "TheCodeSaiyan-LAP",lines: 47,  parsed: 47,  unknown: 0,  size: "8.9 KB",  status: "ingested", session: "S-179" },
  { id: "BATCH-9342", ts: "4d 20:09",   client: "TheCodeSaiyan-PC", lines: 192, parsed: 188, unknown: 4,  size: "39.4 KB", status: "ingested", session: "S-178" },
  { id: "BATCH-9335", ts: "4d 18:44",   client: "TheCodeSaiyan-PC", lines: 67,  parsed: 67,  unknown: 0,  size: "13.1 KB", status: "ingested", session: "S-178" },
  { id: "BATCH-9322", ts: "5d 23:58",   client: "TheCodeSaiyan-PC", lines: 142, parsed: 140, unknown: 2,  size: "28.7 KB", status: "ingested", session: "S-177" },
];

const BATCH_DETAIL_SAMPLE = [
  { ln: 1,  ts: "21:14:08.214", level: "INFO",  type: "quantum_target_selected",  raw: '<Quantum Travel> target="Crusader · Orison" src="ARC-L1" actor=TheCodeSaiyan' },
  { ln: 2,  ts: "21:14:08.302", level: "INFO",  type: "actor_state_change",       raw: '<Actor State> TheCodeSaiyan state=quantum_target' },
  { ln: 3,  ts: "21:14:08.519", level: "DEBUG", type: "vehicle_telemetry",        raw: '<Vehicle> 600i_Explorer power=0.92 fuel=0.64 hull=1.00' },
  { ln: 4,  ts: "21:14:08.781", level: "INFO",  type: "quantum_drive_engage",     raw: '<Quantum Drive> engage spool=4.2s drive_class=A' },
  { ln: 5,  ts: "21:14:09.012", level: "WARN",  type: "(unknown)",                raw: '<RemoteEventLogger> Forced Shutdown by emp_pulse src=npc_ent_4117' },
  { ln: 6,  ts: "21:14:09.214", level: "INFO",  type: "quantum_jump_complete",    raw: '<Quantum Travel> complete dst="Crusader · Orison" dur=42.1s' },
  { ln: 7,  ts: "21:14:09.401", level: "INFO",  type: "instance_change",          raw: '<Instance> moved shard=us-west-2 wave=8' },
  { ln: 8,  ts: "21:14:09.612", level: "DEBUG", type: "actor_state_change",       raw: '<Actor State> TheCodeSaiyan state=in_flight' },
];

const MyLogs = ({ go }) => {
  const [view, setView] = React.useState("overview");
  const [selectedBatch, setSelectedBatch] = React.useState(null);
  const [search, setSearch] = React.useState("");
  const [clientFilter, setClientFilter] = React.useState("all");

  const filtered = LOG_BATCHES.filter(b => {
    if (clientFilter !== "all" && b.client !== clientFilter) return false;
    if (search && !b.id.toLowerCase().includes(search.toLowerCase()) &&
        !b.session.toLowerCase().includes(search.toLowerCase())) return false;
    return true;
  });

  if (selectedBatch) {
    return <BatchDetail id={selectedBatch} onBack={() => setSelectedBatch(null)} />;
  }

  return (
    <Stack gap={20}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>Logs · everything the client has sent</div>
        <Row gap={20} justify="space-between" align="flex-start" style={{ flexWrap: "wrap" }}>
          <div style={{ flex: "1 1 300px" }}>
            <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>
              Transmission archive
            </h1>
            <p style={{ margin: "6px 0 0", color: "var(--fg-muted)", fontSize: 14, maxWidth: 640 }}>
              Every batch the desktop client has uploaded — line-by-line. Use this to audit what's been sent,
              find gaps, or pull the raw payload.
            </p>
          </div>
          <Row gap={8}>
            <Button kind="ghost" icon={<I.download size={13} />}>Export all (NDJSON)</Button>
            <Button kind="primary" icon={<I.signal size={13} />}>Force resync</Button>
          </Row>
        </Row>
      </header>

      <Row gap={6} style={{ flexWrap: "wrap" }}>
        <MetricsTab id="overview" label="Overview" current={view} onClick={setView} />
        <MetricsTab id="batches"  label="Batches" current={view} onClick={setView} count={LOG_BATCHES.length + "+"} />
        <MetricsTab id="health"   label="Health" current={view} onClick={setView} />
        <MetricsTab id="storage"  label="Storage" current={view} onClick={setView} />
      </Row>

      {view === "overview" && <LogsOverview onOpenBatch={setSelectedBatch} />}
      {view === "batches"  && (
        <LogsBatches
          batches={filtered}
          search={search} setSearch={setSearch}
          clientFilter={clientFilter} setClientFilter={setClientFilter}
          onOpenBatch={setSelectedBatch}
        />
      )}
      {view === "health"   && <LogsHealth />}
      {view === "storage"  && <LogsStorage />}
    </Stack>
  );
};

const LogsOverview = ({ onOpenBatch }) => (
  <Stack gap={20}>
    <Row gap={12} data-rsprow="nowrap" style={{ flexWrap: "nowrap" }}>
      {[
        { eyebrow: "Total batches",    value: "1 248", delta: "Across 184 sessions", deltaKind: "neutral" },
        { eyebrow: "Lines transmitted", value: "47.2K", delta: "+2 412 today",     deltaKind: "ok" },
        { eyebrow: "Parser coverage",  value: "97.4%", delta: "1 218 unknowns",     deltaKind: "neutral" },
        { eyebrow: "Storage used",     value: "12.4MB",delta: "of 50MB free quota", deltaKind: "neutral" },
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

    <Row gap={20} data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1.4fr 1fr", alignItems: "stretch" }}>
      <Card eyebrow="Recent batches" title="Last 6">
        <Stack gap={0}>
          {LOG_BATCHES.slice(0, 6).map((b, i, arr) => (
            <button
              key={b.id} type="button"
              onClick={() => onOpenBatch(b.id)}
              style={{
                display: "grid", gridTemplateColumns: "auto 1fr auto auto", gap: 14,
                alignItems: "center", padding: "12px 0",
                borderBottom: i < arr.length - 1 ? "1px solid var(--border)" : "none",
                background: "transparent", border: 0, font: "inherit", color: "inherit",
                textAlign: "left", cursor: "pointer", width: "100%",
              }}
              className="ss-batch-row"
            >
              <span className="mono" style={{ fontSize: 12, color: "var(--accent)", minWidth: 100 }}>{b.id}</span>
              <div style={{ minWidth: 0 }}>
                <div className="mono" style={{ fontSize: 12, color: "var(--fg-muted)" }}>{b.client} · {b.ts}</div>
                <div style={{ fontSize: 11, color: "var(--fg-dim)", marginTop: 2 }}>
                  <span className="mono">{b.lines}</span> lines · <span className="mono">{b.parsed}</span> parsed
                  {b.unknown > 0 && <span style={{ color: "var(--warn)" }}> · {b.unknown} unknown</span>}
                </div>
              </div>
              <span className="mono" style={{ fontSize: 11, color: "var(--fg-dim)" }}>{b.size}</span>
              {b.status === "failed"
                ? <Badge kind="danger" dot>Failed</Badge>
                : <Badge kind="ok" dot>OK</Badge>
              }
            </button>
          ))}
        </Stack>
      </Card>

      <Card eyebrow="Parser breakdown" title="What's getting recognised">
        <Stack gap={12}>
          {[
            { label: "Cleanly parsed",  count: 45_984, pct: 97.4, color: "var(--ok)" },
            { label: "Unknown patterns", count: 1_218, pct: 2.6, color: "var(--warn)" },
            { label: "Malformed",        count: 14,    pct: 0.03, color: "var(--danger)" },
          ].map((p, i) => (
            <div key={i}>
              <Row justify="space-between" style={{ marginBottom: 4 }}>
                <span style={{ fontSize: 13 }}>{p.label}</span>
                <span className="mono" style={{ fontSize: 12, color: "var(--fg-muted)" }}>
                  {p.count.toLocaleString()} <span style={{ color: "var(--fg-dim)" }}>· {p.pct}%</span>
                </span>
              </Row>
              <div style={{ height: 6, background: "var(--bg)", borderRadius: 3, overflow: "hidden" }}>
                <div style={{ height: "100%", width: `${Math.max(p.pct, 0.5)}%`, background: p.color }} />
              </div>
            </div>
          ))}
          <div style={{
            marginTop: 4, padding: "10px 12px",
            background: "var(--bg)", border: "1px solid var(--border)",
            borderRadius: "var(--r-sm)", fontSize: 12, color: "var(--fg-muted)",
          }}>
            <span style={{ color: "var(--accent)" }}>1 218 unknowns</span> — the client has flagged
            patterns it can't classify. Review them and submit useful ones to the network.
            <a href="#" style={{ color: "var(--accent)", display: "block", marginTop: 4 }}>Review unknowns →</a>
          </div>
        </Stack>
      </Card>
    </Row>

    <Card eyebrow="Transmission timeline · last 24h" title="Throughput">
      <div style={{
        display: "grid", gridTemplateColumns: "repeat(48, 1fr)", gap: 2,
        height: 80, alignItems: "end",
      }}>
        {Array.from({ length: 48 }, (_, i) => {
          // simulate activity — quiet during day, peaks evening
          const hr = (i / 2) % 24;
          const base = (hr >= 18 && hr < 24) ? 0.7 : (hr >= 12 && hr < 18) ? 0.25 : (hr >= 8 && hr < 12) ? 0.1 : 0.05;
          const v = Math.max(0.04, base + (Math.sin(i * 0.7) * 0.15) + (Math.random() * 0.1));
          const isPeak = v > 0.6;
          return (
            <div key={i} style={{
              height: `${Math.min(100, v * 100)}%`,
              background: isPeak ? "var(--accent)" : "var(--accent-soft)",
              borderRadius: 1.5,
              opacity: isPeak ? 0.9 : 0.6,
            }} />
          );
        })}
      </div>
      <Row justify="space-between" style={{ marginTop: 10, fontSize: 11, color: "var(--fg-dim)" }}>
        <span className="mono">24h ago</span>
        <span className="mono">12h ago</span>
        <span className="mono">Now</span>
      </Row>
    </Card>
  </Stack>
);

const LogsBatches = ({ batches, search, setSearch, clientFilter, setClientFilter, onOpenBatch }) => (
  <Card
    eyebrow={`${batches.length} batches`}
    title="All transmissions"
    footer={<span style={{ fontSize: 12, color: "var(--fg-dim)" }}>Showing most recent. <a href="#" style={{ color: "var(--accent)" }}>Load older →</a></span>}
  >
    <Row gap={10} style={{ marginBottom: 14, flexWrap: "wrap" }}>
      <input
        type="text"
        placeholder="Search by batch ID or session…"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        className="ss-input mono"
        style={{ flex: "1 1 240px", fontSize: 13 }}
      />
      <select
        value={clientFilter}
        onChange={(e) => setClientFilter(e.target.value)}
        className="ss-input"
        style={{ fontSize: 13 }}
      >
        <option value="all">All clients</option>
        <option value="TheCodeSaiyan-PC">TheCodeSaiyan-PC</option>
        <option value="TheCodeSaiyan-LAP">TheCodeSaiyan-LAP</option>
      </select>
    </Row>

    <div className="ss-table-wrap">
      <table className="ss-table" style={{ fontSize: 12 }}>
        <thead>
          <tr>
            <th style={{ textAlign: "left" }}>Batch ID</th>
            <th style={{ textAlign: "left" }}>When</th>
            <th style={{ textAlign: "left" }}>Client</th>
            <th style={{ textAlign: "left" }}>Session</th>
            <th style={{ textAlign: "right" }}>Lines</th>
            <th style={{ textAlign: "right" }}>Parsed</th>
            <th style={{ textAlign: "right" }}>Unknown</th>
            <th style={{ textAlign: "right" }}>Size</th>
            <th style={{ textAlign: "right" }}>Status</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {batches.map(b => (
            <tr
              key={b.id}
              onClick={() => onOpenBatch(b.id)}
              style={{ cursor: "pointer" }}
            >
              <td><span className="mono" style={{ color: "var(--accent)" }}>{b.id}</span></td>
              <td><span className="mono" style={{ color: "var(--fg-muted)" }}>{b.ts}</span></td>
              <td><span className="mono" style={{ color: "var(--fg-muted)" }}>{b.client}</span></td>
              <td><span className="mono" style={{ color: "var(--fg-dim)" }}>{b.session}</span></td>
              <td style={{ textAlign: "right" }} className="mono">{b.lines}</td>
              <td style={{ textAlign: "right" }} className="mono" >{b.parsed}</td>
              <td style={{ textAlign: "right" }}>
                <span className="mono" style={{ color: b.unknown > 0 ? "var(--warn)" : "var(--fg-dim)" }}>{b.unknown}</span>
              </td>
              <td style={{ textAlign: "right" }} className="mono">{b.size}</td>
              <td style={{ textAlign: "right" }}>
                {b.status === "failed"
                  ? <Badge kind="danger" dot>Failed</Badge>
                  : <Badge kind="ok" dot>OK</Badge>}
              </td>
              <td style={{ textAlign: "right" }}>
                <I.arrow size={12} style={{ color: "var(--fg-dim)" }} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  </Card>
);

const LogsHealth = () => (
  <Stack gap={20}>
    <Row gap={12} data-rsprow="nowrap" style={{ flexWrap: "nowrap" }}>
      {[
        { eyebrow: "Ingestion success", value: "99.92%", delta: "1 failed in 30d", deltaKind: "ok" },
        { eyebrow: "Avg batch size",    value: "126 ln", delta: "Healthy spread",  deltaKind: "neutral" },
        { eyebrow: "Avg latency",       value: "1.4s",   delta: "Client → server", deltaKind: "neutral" },
        { eyebrow: "Sequence gaps",     value: "1",      delta: "Auto-recovered",  deltaKind: "neutral" },
      ].map((s, i) => (
        <div key={i} className="ss-card" style={{ flex: "1 1 200px", padding: "18px 20px" }}>
          <div className="ss-eyebrow">{s.eyebrow}</div>
          <div className="mono" style={{ fontSize: 28, fontWeight: 500, letterSpacing: "-0.015em", margin: "8px 0 6px", color: "var(--fg)" }}>{s.value}</div>
          <div style={{ color: s.deltaKind === "ok" ? "var(--ok)" : "var(--fg-dim)", fontSize: 12 }}>{s.delta}</div>
        </div>
      ))}
    </Row>

    <Row gap={20} data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", alignItems: "stretch" }}>
      <Card eyebrow="Per-client health" title="By machine">
        <Stack gap={14}>
          {[
            { name: "TheCodeSaiyan-PC", batches: 941, success: 100, latency: "1.2s", lastSeen: "12s ago", state: "online" },
            { name: "TheCodeSaiyan-LAP", batches: 307, success: 99.7, latency: "1.8s", lastSeen: "2h ago", state: "idle" },
          ].map((c, i, arr) => (
            <div key={c.name} style={{
              paddingBottom: i < arr.length - 1 ? 14 : 0,
              borderBottom: i < arr.length - 1 ? "1px solid var(--border)" : "none",
            }}>
              <Row justify="space-between" style={{ marginBottom: 8 }}>
                <span className="mono" style={{ fontSize: 13, color: "var(--fg)" }}>{c.name}</span>
                <Badge kind={c.state === "online" ? "ok" : "neutral"} dot>{c.state}</Badge>
              </Row>
              <KV items={[
                ["Batches sent",  <span className="mono">{c.batches}</span>],
                ["Success rate",  <span className="mono">{c.success}%</span>],
                ["Avg latency",   <span className="mono">{c.latency}</span>],
                ["Last seen",     <span className="mono">{c.lastSeen}</span>],
              ]} />
            </div>
          ))}
        </Stack>
      </Card>

      <Card eyebrow="Recent issues · 30 days" title="What went wrong">
        <Stack gap={12}>
          {[
            { ts: "Yest 22:14", batch: "BATCH-9385", title: "Sequence gap detected", body: "Lines 9410–9419 missing from TheCodeSaiyan-LAP. Auto-resync requested.", kind: "warn" },
            { ts: "8d ago",     batch: "BATCH-9101", title: "Upload retry succeeded", body: "Batch initially failed with 502. Resent on backoff.", kind: "neutral" },
            { ts: "21d ago",    batch: "BATCH-8714", title: "Clock drift detected",  body: "Client clock 14s ahead of server. Re-aligned.", kind: "neutral" },
          ].map((i, idx, arr) => (
            <div key={idx} style={{
              paddingBottom: idx < arr.length - 1 ? 12 : 0,
              borderBottom: idx < arr.length - 1 ? "1px solid var(--border)" : "none",
            }}>
              <Row gap={8} style={{ marginBottom: 4 }}>
                <Badge kind={i.kind} dot>{i.title}</Badge>
                <span style={{ flex: 1 }} />
                <span className="mono" style={{ fontSize: 11, color: "var(--fg-dim)" }}>{i.ts}</span>
              </Row>
              <div style={{ fontSize: 12, color: "var(--fg-muted)", marginBottom: 4 }}>{i.body}</div>
              <span className="mono" style={{ fontSize: 11, color: "var(--accent)" }}>{i.batch}</span>
            </div>
          ))}
        </Stack>
      </Card>
    </Row>
  </Stack>
);

const LogsStorage = () => (
  <Stack gap={20}>
    <Card eyebrow="Storage usage" title="What you're holding">
      <Row gap={20} align="center" style={{ marginBottom: 18, flexWrap: "wrap" }}>
        <div style={{ flex: "1 1 200px" }}>
          <div className="mono" style={{ fontSize: 36, fontWeight: 500, color: "var(--fg)" }}>12.4 MB</div>
          <div style={{ fontSize: 12, color: "var(--fg-muted)" }}>of 50 MB free quota · 24.8% used</div>
        </div>
        <Button kind="ghost" icon={<I.heart size={13} />}>Upgrade for unlimited</Button>
      </Row>
      <div style={{ height: 10, background: "var(--bg)", borderRadius: 5, overflow: "hidden", marginBottom: 16 }}>
        <div style={{ height: "100%", width: "24.8%", background: "linear-gradient(90deg, var(--accent), color-mix(in oklab, var(--accent) 70%, var(--ok)))" }} />
      </div>
      <Stack gap={10}>
        {[
          { label: "Parsed events",      size: "8.1 MB",  pct: 65 },
          { label: "Raw log archive",    size: "3.2 MB",  pct: 26 },
          { label: "Sessions + meta",    size: "0.8 MB",  pct: 6  },
          { label: "Unknowns + dropped", size: "0.3 MB",  pct: 3  },
        ].map((b, i) => (
          <div key={i}>
            <Row justify="space-between" style={{ marginBottom: 4 }}>
              <span style={{ fontSize: 13 }}>{b.label}</span>
              <span className="mono" style={{ fontSize: 12, color: "var(--fg-muted)" }}>{b.size}</span>
            </Row>
            <div style={{ height: 4, background: "var(--bg)", borderRadius: 2, overflow: "hidden" }}>
              <div style={{ height: "100%", width: `${b.pct}%`, background: "var(--accent)", opacity: 0.4 + (b.pct / 100) * 0.5 }} />
            </div>
          </div>
        ))}
      </Stack>
    </Card>

    <Row gap={20} data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", alignItems: "stretch" }}>
      <Card eyebrow="Retention policy" title="How long we keep this">
        <KV items={[
          ["Parsed events",   "12 months · free tier"],
          ["Raw log archive", "90 days · then dropped"],
          ["Sessions + meta", "12 months"],
          ["Unknowns",        "Until reviewed or 30d"],
          ["After cancel",    "30-day grace, then purged"],
        ]} />
        <div style={{
          marginTop: 14, padding: "10px 12px",
          background: "var(--accent-soft)", border: "1px solid var(--accent)",
          borderRadius: "var(--r-sm)", fontSize: 12, color: "var(--fg)",
        }}>
          Donate monthly to extend retention to <span className="mono">5 years</span>. <a href="#" style={{ color: "var(--accent)" }}>See plans →</a>
        </div>
      </Card>

      <Card eyebrow="Data control" title="Yours to keep — or kill">
        <Stack gap={10}>
          <Button kind="primary" icon={<I.download size={13} />}>Export everything (NDJSON)</Button>
          <Button kind="ghost"   icon={<I.download size={13} />}>Export raw archive (.zip)</Button>
          <Button kind="ghost"   icon={<I.download size={13} />}>Export parsed events (CSV)</Button>
          <div style={{
            marginTop: 6, padding: "10px 12px",
            background: "var(--bg)", border: "1px solid var(--border)",
            borderRadius: "var(--r-sm)", fontSize: 12, color: "var(--fg-muted)",
          }}>
            Exports include everything the client has ever sent on your behalf. Nothing redacted, nothing held back.
          </div>
          <Button kind="ghost" icon={<I.trash size={13} />} style={{ borderColor: "var(--danger)", color: "var(--danger)" }}>
            Purge all logs older than 30 days
          </Button>
        </Stack>
      </Card>
    </Row>
  </Stack>
);

const BatchDetail = ({ id, onBack }) => {
  const b = LOG_BATCHES.find(x => x.id === id) || LOG_BATCHES[0];

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
          <I.arrow size={12} style={{ transform: "rotate(180deg)" }} /> All batches
        </button>
      </Row>

      <header>
        <Row gap={10} style={{ marginBottom: 10 }}>
          <span className="mono" style={{ fontSize: 12, color: "var(--fg-dim)" }}>{b.id}</span>
          {b.status === "failed"
            ? <Badge kind="danger" dot>Failed · awaiting retry</Badge>
            : <Badge kind="ok" dot>Ingested cleanly</Badge>}
        </Row>
        <h1 className="mono" style={{ margin: 0, fontSize: 26, fontWeight: 500, letterSpacing: "-0.01em", color: "var(--fg)" }}>
          {b.lines} lines · {b.size}
        </h1>
        <p style={{ margin: "8px 0 0", color: "var(--fg-muted)", fontSize: 14 }}>
          From <span className="mono" style={{ color: "var(--fg)" }}>{b.client}</span>,
          captured <span className="mono">{b.ts}</span>, session <span className="mono">{b.session}</span>.
        </p>
      </header>

      <Row gap={20} data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "2fr 1fr", alignItems: "stretch" }}>
        <Card eyebrow="Lines · sample" title="Raw payload" footer={<a href="#" style={{ fontSize: 12, color: "var(--accent)" }}>Show all {b.lines} lines →</a>}>
          <div style={{ overflow: "auto" }}>
            <table className="ss-table mono" style={{ fontSize: 11 }}>
              <thead>
                <tr>
                  <th style={{ textAlign: "right", width: 40 }}>#</th>
                  <th style={{ textAlign: "left" }}>Timestamp</th>
                  <th style={{ textAlign: "left" }}>Lvl</th>
                  <th style={{ textAlign: "left" }}>Type</th>
                  <th style={{ textAlign: "left" }}>Raw</th>
                </tr>
              </thead>
              <tbody>
                {BATCH_DETAIL_SAMPLE.map((r) => (
                  <tr key={r.ln}>
                    <td style={{ textAlign: "right", color: "var(--fg-dim)" }}>{r.ln}</td>
                    <td style={{ color: "var(--fg-dim)" }}>{r.ts}</td>
                    <td style={{
                      color: r.level === "WARN" ? "var(--warn)"
                           : r.level === "DEBUG" ? "var(--fg-dim)" : "var(--fg-muted)"
                    }}>{r.level}</td>
                    <td style={{ color: r.type === "(unknown)" ? "var(--warn)" : "var(--accent)" }}>{r.type}</td>
                    <td style={{ color: "var(--fg-muted)", whiteSpace: "nowrap" }}>{r.raw}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>

        <Stack gap={20}>
          <Card eyebrow="Batch metrics" title="Numbers">
            <KV items={[
              ["Lines",          <span className="mono">{b.lines}</span>],
              ["Parsed",         <span className="mono">{b.parsed} <span style={{ color: "var(--ok)" }}>({((b.parsed / b.lines) * 100).toFixed(1)}%)</span></span>],
              ["Unknown",        <span className="mono" style={{ color: b.unknown > 0 ? "var(--warn)" : "inherit" }}>{b.unknown}</span>],
              ["Compressed",     <span className="mono">{b.size}</span>],
              ["Uncompressed",   <span className="mono">~{(parseFloat(b.size) * 4.2).toFixed(1)} KB</span>],
              ["Upload latency", <span className="mono">1.4s</span>],
              ["Hash",           <span className="mono" style={{ fontSize: 11 }}>0xa4f8…2c14</span>],
            ]} />
          </Card>

          <Card eyebrow="Actions" title="On this batch">
            <Stack gap={8}>
              <Button kind="ghost" icon={<I.download size={13} />}>Download as JSON</Button>
              <Button kind="ghost" icon={<I.eye size={13} />}>View {b.unknown} unknowns</Button>
              {b.status === "failed" && (
                <Button kind="primary" icon={<I.signal size={13} />}>Retry upload now</Button>
              )}
              <Button kind="ghost" icon={<I.trash size={13} />} style={{ borderColor: "var(--danger)", color: "var(--danger)" }}>
                Delete this batch
              </Button>
            </Stack>
          </Card>
        </Stack>
      </Row>
    </Stack>
  );
};
