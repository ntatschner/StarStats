/* Tray UI — Logs view: browse the local SQLite-stored events */

const { useState: useLogsState, useMemo: useLogsMemo } = React;

// Mock: ~140 local events the client has tailed and persisted
const MOCK_LOCAL_EVENTS = (() => {
  const types = [
    { type: 'legacy_login',             tone: 'ok',     summary: (i) => `Login as Caelum (session #${1200 + i})` },
    { type: 'quantum_target_selected',  tone: 'accent', summary: (i) => ['Stanton → Crusader','Stanton → Hurston','Stanton → ArcCorp','Stanton → microTech'][i % 4] },
    { type: 'vehicle_stowed',           tone: 'info',   summary: (i) => ['Drake Cutter — Port Olisar','Aegis Avenger Titan — Lorville','RSI Aurora MR — Area18','Anvil C8X — New Babbage'][i % 4] },
    { type: 'actor_death',              tone: 'danger', summary: (i) => ['Killed by NineTails Rifleman','Killed by environmental hazard','Killed by player [Vexel_77]','Killed by friendly fire'][i % 4] },
    { type: 'vehicle_destruction',      tone: 'danger', summary: (i) => ['Soft death — Avenger Titan','Hard death — Cutter','Disabled — Aurora MR','Hard death — C8X'][i % 4] },
    { type: 'join_pu',                  tone: 'ok',     summary: (i) => ['Joined PU shard us-east-1-a','Joined PU shard eu-central-1-c','Joined PU shard us-west-2-b'][i % 3] },
    { type: 'mission_accepted',         tone: 'info',   summary: (i) => ['Bounty: Headhunter','Cargo: Hauling Contract','Mercenary: ECN Alert','Investigation: Shutdown'][i % 4] },
    { type: 'mission_completed',        tone: 'ok',     summary: (i) => ['Completed bounty (+12,500 aUEC)','Completed haul (+8,200 aUEC)','Completed merc (+24,000 aUEC)'][i % 3] },
  ];
  const out = [];
  let now = Date.now();
  for (let i = 0; i < 142; i++) {
    const t = types[Math.floor(Math.random() * types.length)];
    now -= Math.floor(20_000 + Math.random() * 240_000); // 20s-4m apart
    out.push({
      id: 1000 + (142 - i),
      timestamp: new Date(now).toISOString(),
      event_type: t.type,
      tone: t.tone,
      summary: t.summary(i),
      synced: Math.random() > 0.04,
      raw_line: `[${new Date(now).toISOString()}] <${t.type}> ${t.summary(i).replace(/\s+/g, '_').toLowerCase()}_payload data=...`,
    });
  }
  return out;
})();

const TONE_VAR = {
  ok: 'var(--ok)',
  accent: 'var(--accent)',
  info: 'var(--info)',
  danger: 'var(--danger)',
  warn: 'var(--warn)',
  dim: 'var(--fg-dim)',
};

function fmtClock(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleTimeString();
}
function fmtDate(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}
function ageLabel(iso) {
  const ms = Date.now() - new Date(iso).getTime();
  if (ms < 60_000) return `${Math.floor(ms / 1000)}s ago`;
  if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ago`;
  if (ms < 86_400_000) return `${Math.floor(ms / 3_600_000)}h ago`;
  return `${Math.floor(ms / 86_400_000)}d ago`;
}

// Group events into day buckets (today / yesterday / Apr 28 etc)
function groupByDay(events) {
  const groups = new Map();
  const today = new Date(); today.setHours(0,0,0,0);
  const tStamp = today.getTime();
  for (const e of events) {
    const d = new Date(e.timestamp); d.setHours(0,0,0,0);
    const key = d.getTime();
    let label;
    if (key === tStamp) label = 'Today';
    else if (key === tStamp - 86_400_000) label = 'Yesterday';
    else label = d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
    if (!groups.has(key)) groups.set(key, { label, items: [] });
    groups.get(key).items.push(e);
  }
  return [...groups.values()];
}

function LogsPane() {
  const [query, setQuery] = useLogsState('');
  const [activeType, setActiveType] = useLogsState('all');
  const [selectedId, setSelectedId] = useLogsState(null);
  const [showRaw, setShowRaw] = useLogsState(false);

  const allTypes = useLogsMemo(() => {
    const m = new Map();
    for (const e of MOCK_LOCAL_EVENTS) m.set(e.event_type, (m.get(e.event_type) || 0) + 1);
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  }, []);

  const filtered = useLogsMemo(() => {
    const q = query.trim().toLowerCase();
    return MOCK_LOCAL_EVENTS.filter((e) => {
      if (activeType !== 'all' && e.event_type !== activeType) return false;
      if (q && !(e.event_type.includes(q) || e.summary.toLowerCase().includes(q))) return false;
      return true;
    });
  }, [query, activeType]);

  const grouped = useLogsMemo(() => groupByDay(filtered), [filtered]);
  const selected = useLogsMemo(
    () => MOCK_LOCAL_EVENTS.find((e) => e.id === selectedId) || null,
    [selectedId]
  );

  const totalCount = MOCK_LOCAL_EVENTS.length;
  const syncedCount = MOCK_LOCAL_EVENTS.filter((e) => e.synced).length;
  const unsyncedCount = totalCount - syncedCount;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* Headline strip */}
      <div style={{ display: 'flex', gap: 8 }}>
        <StatPill label="Stored" value={totalCount.toLocaleString()} tone="accent" />
        <StatPill label="Synced" value={syncedCount.toLocaleString()} tone="ok" />
        <StatPill label="Pending" value={unsyncedCount.toLocaleString()} tone={unsyncedCount > 0 ? 'warn' : 'default'} />
        <StatPill label="DB size" value="2.4 MB" />
      </div>

      {/* Search bar */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 8,
        background: 'var(--surface)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--r-md)',
        padding: '6px 10px',
      }}>
        <span style={{ color: 'var(--fg-dim)', fontSize: 13, fontFamily: 'var(--font-mono)' }}>⌕</span>
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter by type or summary…"
          style={{
            flex: 1, background: 'transparent', border: 'none', outline: 'none',
            color: 'var(--fg)', fontFamily: 'var(--font-mono)', fontSize: 12,
            padding: '4px 0',
          }}
        />
        {query && (
          <button
            type="button"
            onClick={() => setQuery('')}
            style={{
              background: 'transparent', border: 'none', color: 'var(--fg-dim)',
              cursor: 'pointer', fontSize: 11, padding: 0,
            }}
          >clear</button>
        )}
        <span style={{
          color: 'var(--fg-dim)', fontSize: 11, fontFamily: 'var(--font-mono)',
          paddingLeft: 8, borderLeft: '1px solid var(--border)',
        }}>{filtered.length} / {totalCount}</span>
      </div>

      {/* Type pill row */}
      <div style={{
        display: 'flex', flexWrap: 'wrap', gap: 4,
        marginTop: -4,
      }}>
        <TypePill label="All" count={totalCount} active={activeType === 'all'} onClick={() => setActiveType('all')} />
        {allTypes.map(([type, count]) => (
          <TypePill
            key={type} label={type} count={count}
            active={activeType === type}
            onClick={() => setActiveType(type)}
          />
        ))}
      </div>

      {/* Grouped list */}
      {grouped.length === 0 ? (
        <TrayCard>
          <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13, textAlign: 'center', padding: '12px 0' }}>
            Scope is clear. No events match this filter.
          </p>
        </TrayCard>
      ) : (
        grouped.map((g) => (
          <TrayCard key={g.label} title={g.label} kicker={`${g.items.length} events`}>
            <ol style={{
              listStyle: 'none', margin: 0, padding: 0,
              display: 'flex', flexDirection: 'column', gap: 1,
            }}>
              {g.items.map((e) => (
                <li
                  key={e.id}
                  onClick={() => setSelectedId(e.id)}
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '60px 170px 1fr auto',
                    gap: 10, alignItems: 'baseline',
                    padding: '5px 8px',
                    borderLeft: `2px solid ${TONE_VAR[e.tone]}`,
                    fontSize: 12,
                    cursor: 'pointer',
                    background: selectedId === e.id ? 'var(--surface-2)' : 'transparent',
                    transition: 'background 120ms',
                  }}
                  onMouseEnter={(ev) => { if (selectedId !== e.id) ev.currentTarget.style.background = 'var(--surface-2)'; }}
                  onMouseLeave={(ev) => { if (selectedId !== e.id) ev.currentTarget.style.background = 'transparent'; }}
                >
                  <span style={{ color: 'var(--fg-dim)', fontFamily: 'var(--font-mono)' }}>{fmtClock(e.timestamp)}</span>
                  <code style={{
                    color: TONE_VAR[e.tone], fontSize: 11, textTransform: 'uppercase',
                    letterSpacing: '0.04em', fontFamily: 'var(--font-mono)',
                    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                  }}>{e.event_type}</code>
                  <span style={{ color: 'var(--fg)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {e.summary}
                  </span>
                  <span style={{
                    fontSize: 10, fontFamily: 'var(--font-mono)',
                    color: e.synced ? 'var(--fg-dim)' : 'var(--warn)',
                    letterSpacing: '0.06em', textTransform: 'uppercase',
                  }}>{e.synced ? '✓' : '↑'}</span>
                </li>
              ))}
            </ol>
          </TrayCard>
        ))
      )}

      {/* Drawer / detail panel */}
      {selected && (
        <div
          onClick={() => setSelectedId(null)}
          style={{
            position: 'fixed', inset: 0,
            background: 'rgba(0, 0, 0, 0.55)',
            zIndex: 100,
            display: 'flex', alignItems: 'flex-end', justifyContent: 'center',
            animation: 'fadeIn 180ms',
          }}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              width: '100%', maxWidth: 720,
              background: 'var(--bg-elev)',
              borderTop: '1px solid var(--border-strong)',
              borderTopLeftRadius: 'var(--r-lg)',
              borderTopRightRadius: 'var(--r-lg)',
              padding: 20,
              maxHeight: '70vh', overflowY: 'auto',
              animation: 'slideUp 240ms var(--ease-out)',
            }}
          >
            <div style={{
              display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between',
              gap: 12, marginBottom: 14,
            }}>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                  <StatusDot tone={selected.tone === 'danger' ? 'danger' : selected.tone === 'warn' ? 'warn' : 'ok'} />
                  <code style={{
                    color: TONE_VAR[selected.tone],
                    fontFamily: 'var(--font-mono)', fontSize: 14, fontWeight: 600,
                    textTransform: 'uppercase', letterSpacing: '0.04em',
                  }}>{selected.event_type}</code>
                </div>
                <div style={{ fontSize: 14, color: 'var(--fg)' }}>{selected.summary}</div>
              </div>
              <button
                type="button"
                onClick={() => setSelectedId(null)}
                style={{
                  background: 'transparent', border: '1px solid var(--border-strong)',
                  color: 'var(--fg-muted)', borderRadius: 'var(--r-sm)',
                  padding: '4px 10px', fontSize: 12, cursor: 'pointer',
                  fontFamily: 'inherit',
                }}
              >Close</button>
            </div>

            <dl style={{
              display: 'grid', gridTemplateColumns: '120px 1fr',
              gap: '6px 12px', margin: '0 0 14px',
            }}>
              <KV label="Event ID" value={`#${selected.id}`} mono />
              <KV label="Captured" value={`${fmtDate(selected.timestamp)} · ${fmtClock(selected.timestamp)} (${ageLabel(selected.timestamp)})`} mono />
              <KV label="Sync state" value={
                <span style={{
                  display: 'inline-flex', alignItems: 'center', gap: 6,
                  fontFamily: 'var(--font-mono)', fontSize: 12,
                  color: selected.synced ? 'var(--ok)' : 'var(--warn)',
                }}>
                  <StatusDot tone={selected.synced ? 'ok' : 'warn'} />
                  {selected.synced ? 'Synced to remote' : 'Pending — will retry next batch'}
                </span>
              } />
              <KV label="Source" value="LIVE/Game.log" mono dim />
            </dl>

            <div style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              marginBottom: 6,
            }}>
              <div style={{
                fontSize: 10, fontWeight: 600, color: 'var(--fg-muted)',
                textTransform: 'uppercase', letterSpacing: '0.12em',
              }}>Raw line</div>
              <button
                type="button"
                onClick={() => setShowRaw((v) => !v)}
                style={{
                  background: 'transparent', border: '1px solid var(--border-strong)',
                  color: 'var(--fg-muted)', borderRadius: 'var(--r-xs)',
                  padding: '2px 8px', fontSize: 10, cursor: 'pointer',
                  fontFamily: 'inherit',
                }}
              >{showRaw ? 'Hide' : 'Show'}</button>
            </div>
            {showRaw && (
              <pre style={{
                margin: 0, padding: '10px 12px',
                background: 'var(--bg)', border: '1px solid var(--border)',
                borderRadius: 'var(--r-sm)',
                color: 'var(--fg-muted)', fontSize: 11,
                fontFamily: 'var(--font-mono)',
                whiteSpace: 'pre-wrap', wordBreak: 'break-all',
              }}>{selected.raw_line}</pre>
            )}

            <div style={{
              display: 'flex', gap: 8, marginTop: 14,
              paddingTop: 14, borderTop: '1px solid var(--border)',
            }}>
              <GhostButton type="button">Copy raw line</GhostButton>
              <GhostButton type="button">Mark as noise</GhostButton>
              {!selected.synced && (
                <GhostButton type="button">Retry sync</GhostButton>
              )}
            </div>
          </div>
        </div>
      )}

      <style>{`
        @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
        @keyframes slideUp { from { transform: translateY(20px); opacity: 0; } to { transform: translateY(0); opacity: 1; } }
      `}</style>
    </div>
  );
}

function TypePill({ label, count, active, onClick }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        background: active ? 'var(--accent-soft)' : 'var(--surface)',
        color: active ? 'var(--accent)' : 'var(--fg-muted)',
        border: `1px solid ${active ? 'var(--accent)' : 'var(--border)'}`,
        borderRadius: 'var(--r-pill)',
        padding: '3px 9px',
        fontFamily: 'var(--font-mono)', fontSize: 11,
        cursor: 'pointer', whiteSpace: 'nowrap',
        transition: 'all 120ms',
      }}
    >
      <span>{label}</span>
      <span style={{
        fontSize: 10, color: active ? 'var(--accent)' : 'var(--fg-dim)',
        fontVariantNumeric: 'tabular-nums',
      }}>{count}</span>
    </button>
  );
}

window.LogsPane = LogsPane;
