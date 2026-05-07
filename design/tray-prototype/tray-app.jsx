/* StarStats Tray UI — designed for the Tauri webview.
 * Compact density (640-720px wide), no rail, tight vertical rhythm. */

const { useState, useRef, useEffect, useMemo } = React;

// ---------- Mock data (mirrors api.ts shapes) -------------------
const MOCK_STATUS = {
  tail: {
    current_path: 'C:\\Program Files\\Roberts Space Industries\\StarCitizen\\LIVE\\Game.log',
    bytes_read: 4_823_104,
    lines_processed: 28_491,
    events_recognised: 2_147,
    last_event_at: '2026-05-06T18:42:11Z',
    last_event_type: 'quantum_target_selected',
    lines_structural_only: 312,
    lines_skipped: 84,
    lines_noise: 1_044,
  },
  sync: {
    last_attempt_at: '2026-05-06T18:43:00Z',
    last_success_at: '2026-05-06T18:43:00Z',
    last_error: null,
    batches_sent: 184,
    events_accepted: 2_098,
    events_duplicate: 41,
    events_rejected: 8,
  },
  event_counts: [
    { event_type: 'legacy_login', count: 412 },
    { event_type: 'quantum_target_selected', count: 287 },
    { event_type: 'actor_death', count: 164 },
    { event_type: 'vehicle_stowed', count: 98 },
    { event_type: 'vehicle_destruction', count: 71 },
    { event_type: 'join_pu', count: 54 },
  ],
  total_events: 2_147,
  discovered_logs: [
    { channel: 'LIVE', path: 'C:\\...\\StarCitizen\\LIVE\\Game.log', size_bytes: 4_823_104 },
    { channel: 'PTU',  path: 'C:\\...\\StarCitizen\\PTU\\Game.log', size_bytes: 1_204_388 },
  ],
  account: { auth_lost: false, email_verified: true },
  hangar: {
    last_attempt_at: '2026-05-06T18:30:00Z',
    last_success_at: '2026-05-06T18:30:00Z',
    last_error: null,
    ships_pushed: 23,
    last_skip_reason: null,
  },
};

const MOCK_TIMELINE = [
  { id: 1, timestamp: '2026-05-06T18:42:11Z', event_type: 'quantum_target_selected', summary: 'Stanton → Crusader' },
  { id: 2, timestamp: '2026-05-06T18:38:04Z', event_type: 'vehicle_stowed', summary: 'Drake Cutter — Port Olisar' },
  { id: 3, timestamp: '2026-05-06T18:21:55Z', event_type: 'actor_death', summary: 'Killed by NineTails Rifleman' },
  { id: 4, timestamp: '2026-05-06T18:14:30Z', event_type: 'join_pu', summary: 'Joined PU shard us-east-1-a' },
  { id: 5, timestamp: '2026-05-06T18:14:02Z', event_type: 'legacy_login', summary: 'Login as Caelum' },
];

const MOCK_COVERAGE = {
  recognised: 2_147,
  structural_only: 312,
  skipped: 84,
  noise: 1_044,
  unknowns: [
    {
      log_source: 'CIG-Net', event_name: 'EventReplicationLayer_Spawn',
      occurrences: 184, first_seen: '2026-05-06T17:40:00Z', last_seen: '2026-05-06T18:42:00Z',
      sample_line: '[2026-05-06T17:40:00Z] <EventReplicationLayer_Spawn> entity_id=12345 ...',
      sample_body: 'entity_id=12345 class_name=Item_Weapon_Personal_Behring_P4AR archetype=PersonalWeapon spawn_zone=Hurston',
    },
    {
      log_source: 'GameSession', event_name: 'PlayerCommsroom_State',
      occurrences: 47, first_seen: '2026-05-06T18:01:00Z', last_seen: '2026-05-06T18:35:00Z',
      sample_line: '[2026-05-06T18:01:00Z] <PlayerCommsroom_State> ...',
      sample_body: 'state=closed reason=user_dismissed channel_id=44a',
    },
  ],
};

const MOCK_CONFIG = {
  gamelog_path: null,
  remote_sync: {
    enabled: true,
    api_url: 'https://api.example.com',
    claimed_handle: 'Caelum',
    access_token: 'tok_xxxxxxxxxxxx',
    interval_secs: 60,
    batch_size: 200,
  },
  web_origin: 'https://api.example.com',
};

// ---------- Helpers ---------------------------------------------
const fmtBytes = (n) => {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
};
const fmtTime = (iso) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleTimeString();
};
const fmtCovPct = (r, s) => {
  const t = r + s;
  return t === 0 ? '—' : `${((r / t) * 100).toFixed(1)}%`;
};

// ---------- Tray-specific primitives ----------------------------
function TrayCard({ title, kicker, right, children, mono = false }) {
  return (
    <section style={{
      background: 'var(--surface)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--r-md)',
      padding: '14px 16px',
    }}>
      {(title || right) && (
        <header style={{
          display: 'flex', alignItems: 'baseline', justifyContent: 'space-between',
          gap: 12, marginBottom: 10,
        }}>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
            {title && (
              <h2 style={{
                margin: 0, fontSize: 11, fontWeight: 600, color: 'var(--fg-muted)',
                textTransform: 'uppercase', letterSpacing: '0.12em',
                fontFamily: mono ? 'var(--font-mono)' : 'var(--font-sans)',
              }}>{title}</h2>
            )}
            {kicker && (
              <span style={{ fontSize: 11, color: 'var(--fg-dim)', fontFamily: 'var(--font-mono)' }}>
                {kicker}
              </span>
            )}
          </div>
          {right}
        </header>
      )}
      {children}
    </section>
  );
}

function KV({ label, value, mono = false, dim = false }) {
  return (
    <>
      <dt style={{ color: 'var(--fg-muted)', fontSize: 12 }}>{label}</dt>
      <dd style={{
        margin: 0,
        fontVariantNumeric: 'tabular-nums',
        fontSize: 13,
        color: dim ? 'var(--fg-dim)' : 'var(--fg)',
        fontFamily: mono ? 'var(--font-mono)' : 'var(--font-sans)',
        wordBreak: mono ? 'break-all' : 'normal',
      }}>{value}</dd>
    </>
  );
}

function StatPill({ label, value, tone = 'default' }) {
  const colors = {
    default: 'var(--fg)',
    ok: 'var(--ok)',
    warn: 'var(--warn)',
    danger: 'var(--danger)',
    accent: 'var(--accent)',
  };
  return (
    <div style={{
      flex: '1 1 0', minWidth: 0,
      background: 'var(--surface-2)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--r-sm)',
      padding: '8px 10px',
    }}>
      <div style={{
        fontSize: 10, color: 'var(--fg-muted)', textTransform: 'uppercase',
        letterSpacing: '0.1em', marginBottom: 3,
      }}>{label}</div>
      <div style={{
        fontFamily: 'var(--font-mono)', fontSize: 16, fontWeight: 600,
        color: colors[tone], fontVariantNumeric: 'tabular-nums',
      }}>{value}</div>
    </div>
  );
}

function StatusDot({ tone = 'ok' }) {
  const c = { ok: 'var(--ok)', warn: 'var(--warn)', danger: 'var(--danger)', dim: 'var(--fg-dim)' }[tone];
  return (
    <span style={{
      display: 'inline-block', width: 8, height: 8, borderRadius: '50%',
      background: c, boxShadow: `0 0 0 3px ${c}22`, flexShrink: 0,
    }} />
  );
}

function Banner({ tone, children, action, onAction }) {
  const tones = {
    warn:   { border: 'var(--warn)',   bg: 'rgba(232, 197, 60, 0.08)',  fg: 'var(--warn)' },
    info:   { border: 'var(--info)',   bg: 'rgba(111, 168, 232, 0.08)', fg: 'var(--info)' },
    danger: { border: 'var(--danger)', bg: 'rgba(232, 103, 76, 0.08)',  fg: 'var(--danger)' },
  };
  const t = tones[tone] || tones.info;
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      gap: 12, padding: '10px 14px', borderRadius: 'var(--r-sm)',
      border: `1px solid ${t.border}`, background: t.bg, color: t.fg,
      fontSize: 13,
    }}>
      <span>{children}</span>
      {action && (
        <button
          type="button"
          onClick={onAction}
          style={{
            background: 'transparent', color: 'inherit',
            border: '1px solid currentColor', borderRadius: 'var(--r-sm)',
            padding: '4px 10px', fontWeight: 600, fontSize: 12,
            cursor: 'pointer', whiteSpace: 'nowrap', fontFamily: 'inherit',
          }}
        >{action}</button>
      )}
    </div>
  );
}

function Field({ label, hint, children }) {
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <span style={{
        fontSize: 10, fontWeight: 600, color: 'var(--fg-muted)',
        textTransform: 'uppercase', letterSpacing: '0.1em',
      }}>{label}</span>
      {children}
      {hint && (
        <small style={{ fontSize: 11, color: 'var(--fg-dim)', lineHeight: 1.4 }}>{hint}</small>
      )}
    </label>
  );
}

const inputStyle = {
  background: 'var(--bg)',
  color: 'var(--fg)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--r-sm)',
  padding: '7px 9px',
  fontFamily: 'var(--font-mono)',
  fontSize: 12,
  outline: 'none',
};

function TextInput(props) {
  return (
    <input {...props} style={{ ...inputStyle, ...(props.style || {}) }} />
  );
}

function PrimaryButton({ children, ...props }) {
  return (
    <button
      {...props}
      style={{
        background: 'var(--accent)', color: 'var(--accent-fg)',
        border: 'none', borderRadius: 'var(--r-sm)',
        padding: '7px 14px', fontWeight: 600, fontSize: 12,
        cursor: props.disabled ? 'not-allowed' : 'pointer',
        opacity: props.disabled ? 0.55 : 1,
        fontFamily: 'inherit', letterSpacing: '0.02em',
        ...(props.style || {}),
      }}
    >{children}</button>
  );
}

function GhostButton({ children, ...props }) {
  return (
    <button
      {...props}
      style={{
        background: 'transparent', color: 'var(--fg-muted)',
        border: '1px solid var(--border-strong)', borderRadius: 'var(--r-sm)',
        padding: '6px 12px', fontWeight: 500, fontSize: 12,
        cursor: 'pointer', fontFamily: 'inherit',
        ...(props.style || {}),
      }}
    >{children}</button>
  );
}

// ---------- Header ----------------------------------------------
function TrayHeader({ view, onView, status }) {
  const live = status?.tail?.current_path !== null;
  return (
    <header style={{
      display: 'grid',
      gridTemplateColumns: 'auto 1fr auto',
      alignItems: 'center',
      gap: 16,
      padding: '12px 16px',
      borderBottom: '1px solid var(--border)',
      background: 'var(--bg-elev)',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <span style={{
          fontFamily: 'var(--font-mono)', fontSize: 16, color: 'var(--accent)',
          fontWeight: 700, letterSpacing: '-0.02em',
        }}>★</span>
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <div style={{
            fontWeight: 700, fontSize: 13, letterSpacing: '0.06em',
            textTransform: 'uppercase',
          }}>STARSTATS</div>
          <div style={{ fontSize: 10, color: 'var(--fg-dim)', letterSpacing: '0.04em' }}>
            Tray client · v0.4.1
          </div>
        </div>
      </div>

      <nav style={{ display: 'flex', gap: 4, justifyContent: 'center' }}>
        {['status', 'logs', 'settings'].map((v) => (
          <button
            key={v}
            type="button"
            onClick={() => onView(v)}
            style={{
              background: view === v ? 'var(--accent-soft)' : 'transparent',
              color: view === v ? 'var(--accent)' : 'var(--fg-muted)',
              border: `1px solid ${view === v ? 'var(--accent)' : 'transparent'}`,
              borderRadius: 'var(--r-sm)',
              padding: '5px 14px',
              fontFamily: 'inherit', fontSize: 12, fontWeight: 600,
              textTransform: 'uppercase', letterSpacing: '0.08em',
              cursor: 'pointer',
            }}
          >{v}</button>
        ))}
      </nav>

      <div style={{
        display: 'flex', alignItems: 'center', gap: 6,
        fontSize: 11, color: 'var(--fg-muted)',
      }}>
        <StatusDot tone={live ? 'ok' : 'dim'} />
        <span style={{ fontFamily: 'var(--font-mono)' }}>
          {live ? 'TAILING' : 'IDLE'}
        </span>
      </div>
    </header>
  );
}

// ---------- Status Pane -----------------------------------------
function StatusPane({ status, coverage, timeline, onGoToSettings }) {
  const { tail, sync, event_counts, total_events, discovered_logs, account, hangar } = status;
  const showAuthLost = account.auth_lost;
  const showEmailUnverified = !showAuthLost && account.email_verified === false;

  // Top-types ranked bar
  const maxCount = Math.max(...event_counts.map((c) => c.count), 1);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {showAuthLost && (
        <Banner tone="warn" action="Re-pair" onAction={onGoToSettings}>
          This device is no longer paired with your account.
        </Banner>
      )}
      {showEmailUnverified && (
        <Banner tone="info">Your Comm-Link isn't verified. Verify it on the web.</Banner>
      )}

      {/* HEADLINE STAT STRIP */}
      <div style={{ display: 'flex', gap: 8 }}>
        <StatPill label="Events" value={total_events.toLocaleString()} tone="accent" />
        <StatPill label="Lines" value={tail.lines_processed.toLocaleString()} />
        <StatPill label="Batches" value={sync.batches_sent.toLocaleString()} />
        <StatPill label="Coverage" value={fmtCovPct(coverage.recognised, coverage.structural_only)} tone="ok" />
      </div>

      {/* TAILING + SYNC, side-by-side */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        <TrayCard title="Tailing" kicker={tail.current_path ? 'LIVE' : 'IDLE'}>
          {tail.current_path ? (
            <dl style={{ display: 'grid', gridTemplateColumns: '90px 1fr', gap: '6px 10px', margin: 0 }}>
              <KV label="Path" value={tail.current_path} mono />
              <KV label="Read" value={fmtBytes(tail.bytes_read)} />
              <KV label="Last event" value={
                tail.last_event_type
                  ? <><code style={{ color: 'var(--accent)', fontSize: 12 }}>{tail.last_event_type}</code> <span style={{ color: 'var(--fg-dim)' }}>· {fmtTime(tail.last_event_at)}</span></>
                  : <span style={{ color: 'var(--fg-dim)' }}>—</span>
              } />
            </dl>
          ) : (
            <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
              Scope is clear. No Game.log discovered yet — set a custom path in Settings.
            </p>
          )}
        </TrayCard>

        <TrayCard
          title="Remote sync"
          right={
            <span style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--ok)' }}>
              <StatusDot tone={sync.last_error ? 'danger' : 'ok'} />
              <span style={{ fontFamily: 'var(--font-mono)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
                {sync.last_error ? 'ERR' : 'OK'}
              </span>
            </span>
          }
        >
          <dl style={{ display: 'grid', gridTemplateColumns: '90px 1fr', gap: '6px 10px', margin: 0 }}>
            <KV label="Last sync" value={sync.last_success_at ? fmtTime(sync.last_success_at) : '—'} mono />
            <KV label="Accepted" value={`${sync.events_accepted.toLocaleString()} / ${(sync.events_accepted + sync.events_duplicate + sync.events_rejected).toLocaleString()}`} mono />
            <KV label="Dup / rej" value={`${sync.events_duplicate} · ${sync.events_rejected}`} mono dim />
          </dl>
        </TrayCard>
      </div>

      {/* TOP TYPES */}
      <TrayCard title="Top event types" kicker={`${event_counts.length} types`}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
          {event_counts.map((c) => {
            const pct = (c.count / maxCount) * 100;
            return (
              <div key={c.event_type} style={{
                display: 'grid', gridTemplateColumns: '1fr auto', gap: 8,
                alignItems: 'center', position: 'relative',
              }}>
                <div style={{ position: 'relative' }}>
                  <div style={{
                    position: 'absolute', inset: 0, background: 'var(--surface-2)',
                    borderRadius: 'var(--r-xs)',
                  }} />
                  <div style={{
                    position: 'absolute', top: 0, bottom: 0, left: 0, width: `${pct}%`,
                    background: 'linear-gradient(90deg, var(--accent-soft) 0%, transparent 100%)',
                    borderRadius: 'var(--r-xs)',
                    borderLeft: '2px solid var(--accent)',
                  }} />
                  <code style={{
                    position: 'relative', display: 'block',
                    padding: '4px 8px', fontSize: 11.5,
                    color: 'var(--fg)', fontFamily: 'var(--font-mono)',
                  }}>{c.event_type}</code>
                </div>
                <span style={{
                  fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--fg-muted)',
                  fontVariantNumeric: 'tabular-nums', minWidth: 44, textAlign: 'right',
                }}>{c.count.toLocaleString()}</span>
              </div>
            );
          })}
        </div>
      </TrayCard>

      {/* TIMELINE */}
      <TrayCard title="Session timeline" kicker={`${timeline.length} entries`}>
        <ol style={{
          listStyle: 'none', margin: 0, padding: 0,
          display: 'flex', flexDirection: 'column', gap: 2,
        }}>
          {timeline.map((e) => {
            const accent =
              e.event_type === 'actor_death' || e.event_type === 'vehicle_destruction' ? 'var(--danger)' :
              e.event_type === 'legacy_login' || e.event_type === 'join_pu' ? 'var(--ok)' :
              e.event_type === 'quantum_target_selected' ? 'var(--accent)' :
              'var(--info)';
            return (
              <li key={e.id} style={{
                display: 'grid',
                gridTemplateColumns: '60px 160px 1fr',
                gap: 10, alignItems: 'baseline',
                padding: '4px 6px',
                borderLeft: `2px solid ${accent}`,
                fontSize: 12,
              }}>
                <span style={{ color: 'var(--fg-dim)', fontFamily: 'var(--font-mono)' }}>{fmtTime(e.timestamp)}</span>
                <code style={{
                  color: accent, fontSize: 11, textTransform: 'uppercase',
                  letterSpacing: '0.04em', fontFamily: 'var(--font-mono)',
                  whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                }}>{e.event_type}</code>
                <span style={{ color: 'var(--fg)' }}>{e.summary}</span>
              </li>
            );
          })}
        </ol>
      </TrayCard>

      {/* COVERAGE */}
      <TrayCard title="Parser coverage">
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 12, marginBottom: 6 }}>
          <span style={{
            fontFamily: 'var(--font-mono)', fontSize: 28, fontWeight: 700,
            color: 'var(--accent)', fontVariantNumeric: 'tabular-nums', lineHeight: 1,
          }}>{fmtCovPct(coverage.recognised, coverage.structural_only)}</span>
          <span style={{ fontSize: 12, color: 'var(--fg-muted)' }}>recognised lines</span>
        </div>
        <div style={{ fontSize: 11, color: 'var(--fg-dim)', fontFamily: 'var(--font-mono)' }}>
          {coverage.recognised.toLocaleString()} ok · {coverage.structural_only.toLocaleString()} unknown · {coverage.noise.toLocaleString()} noise · {coverage.skipped.toLocaleString()} skipped
        </div>

        <details style={{ marginTop: 10 }}>
          <summary style={{
            cursor: 'pointer', color: 'var(--fg-muted)', fontSize: 12,
            padding: '4px 0',
          }}>Unrecognised types ({coverage.unknowns.length})</summary>
          <ul style={{
            listStyle: 'none', padding: 0, margin: '8px 0 0',
            display: 'flex', flexDirection: 'column', gap: 6,
          }}>
            {coverage.unknowns.map((u) => (
              <li key={u.event_name} style={{
                border: '1px solid var(--border)', borderRadius: 'var(--r-sm)',
                padding: '6px 8px', background: 'var(--bg)',
              }}>
                <div style={{
                  display: 'grid', gridTemplateColumns: 'auto 1fr auto auto',
                  gap: 8, alignItems: 'center', fontSize: 11.5,
                }}>
                  <span style={{
                    background: 'var(--accent-soft)', color: 'var(--accent)',
                    fontSize: 10, fontWeight: 700, padding: '1px 6px',
                    borderRadius: 'var(--r-xs)', letterSpacing: '0.04em',
                  }}>{u.log_source}</span>
                  <code style={{ color: 'var(--accent)', fontFamily: 'var(--font-mono)', wordBreak: 'break-all' }}>{u.event_name}</code>
                  <span style={{ color: 'var(--fg-muted)', fontFamily: 'var(--font-mono)', fontVariantNumeric: 'tabular-nums' }}>{u.occurrences}</span>
                  <button type="button" style={{
                    background: 'transparent', border: '1px solid var(--border-strong)',
                    color: 'var(--fg-muted)', borderRadius: 'var(--r-xs)',
                    padding: '1px 6px', fontSize: 10, cursor: 'pointer',
                    fontFamily: 'inherit',
                  }}>ignore</button>
                </div>
              </li>
            ))}
          </ul>
        </details>
      </TrayCard>

      {/* DISCOVERED LOGS */}
      <TrayCard title="Discovered logs" kicker={`${discovered_logs.length} found`}>
        <ul style={{
          listStyle: 'none', margin: 0, padding: 0,
          display: 'flex', flexDirection: 'column', gap: 4,
        }}>
          {discovered_logs.map((l) => (
            <li key={l.path} style={{
              display: 'grid', gridTemplateColumns: 'auto 1fr auto',
              gap: 10, alignItems: 'center', fontSize: 12,
              padding: '4px 0',
            }}>
              <span style={{
                background: l.channel === 'LIVE' ? 'var(--ok)' : 'var(--info)',
                color: 'var(--bg)', fontSize: 10, fontWeight: 700,
                padding: '1px 6px', borderRadius: 'var(--r-xs)', letterSpacing: '0.06em',
              }}>{l.channel}</span>
              <code style={{
                color: 'var(--fg)', fontFamily: 'var(--font-mono)', fontSize: 11,
                overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
              }}>{l.path}</code>
              <span style={{ color: 'var(--fg-muted)', fontFamily: 'var(--font-mono)', fontSize: 11 }}>
                {fmtBytes(l.size_bytes)}
              </span>
            </li>
          ))}
        </ul>
      </TrayCard>

      {/* HANGAR */}
      <TrayCard title="Hangar" kicker={hangar.last_error ? 'ERROR' : 'OK'}>
        <dl style={{ display: 'grid', gridTemplateColumns: '90px 1fr', gap: '6px 10px', margin: 0 }}>
          <KV label="Last sync" value={hangar.last_success_at ? fmtTime(hangar.last_success_at) : '—'} mono />
          <KV label="Ships pushed" value={hangar.ships_pushed.toLocaleString()} mono />
          <KV label="Status" value={
            hangar.last_skip_reason ?? hangar.last_error ?? <span style={{ color: 'var(--ok)' }}>ok</span>
          } />
        </dl>
      </TrayCard>
    </div>
  );
}

// ---------- Settings Pane ---------------------------------------
function SettingsPane({ config, onSave }) {
  const [draft, setDraft] = useState(config);
  const [pairingCode, setPairingCode] = useState('');
  const [cookieConfigured, setCookieConfigured] = useState(true);
  const [cookiePreview, setCookiePreview] = useState('a3f1');
  const [cookieDraft, setCookieDraft] = useState('');

  const updateRemote = (patch) =>
    setDraft((p) => ({ ...p, remote_sync: { ...p.remote_sync, ...patch } }));

  const isPaired = draft.remote_sync.access_token && draft.remote_sync.claimed_handle;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <TrayCard title="Game.log">
        <Field label="Override path" hint="Leave blank to auto-discover the largest LIVE/PTU/EPTU log.">
          <TextInput
            type="text"
            value={draft.gamelog_path ?? ''}
            placeholder="auto-discover"
            onChange={(e) => setDraft({ ...draft, gamelog_path: e.target.value || null })}
          />
        </Field>
      </TrayCard>

      <TrayCard
        title="Remote sync"
        right={
          <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--fg-muted)' }}>
            <input
              type="checkbox"
              checked={draft.remote_sync.enabled}
              onChange={(e) => updateRemote({ enabled: e.target.checked })}
              style={{ accentColor: 'var(--accent)' }}
            />
            <span style={{ textTransform: 'uppercase', letterSpacing: '0.1em' }}>
              {draft.remote_sync.enabled ? 'ON' : 'OFF'}
            </span>
          </label>
        }
      >
        <p style={{ margin: '0 0 12px', color: 'var(--fg-muted)', fontSize: 12, lineHeight: 1.5 }}>
          Pushes structured events to a StarStats API server. Disabled by default — you choose when to share.
        </p>

        <fieldset disabled={!draft.remote_sync.enabled} style={{
          border: 'none', margin: 0, padding: 0,
          opacity: draft.remote_sync.enabled ? 1 : 0.45,
          display: 'flex', flexDirection: 'column', gap: 12,
        }}>
          <Field label="API URL">
            <TextInput
              type="url"
              value={draft.remote_sync.api_url ?? ''}
              placeholder="https://api.example.com"
              onChange={(e) => updateRemote({ api_url: e.target.value || null })}
            />
          </Field>

          <Field label="Hangar">
            {isPaired ? (
              <div style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                gap: 12, padding: '8px 10px',
                background: 'var(--surface-2)', border: '1px solid var(--border)',
                borderRadius: 'var(--r-sm)',
              }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <StatusDot tone="ok" />
                  <div>
                    <div style={{ fontSize: 12, color: 'var(--fg)' }}>
                      Paired as <strong style={{ color: 'var(--accent)' }}>{draft.remote_sync.claimed_handle}</strong>
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--fg-dim)', fontFamily: 'var(--font-mono)' }}>
                      tok_•••• {draft.remote_sync.access_token.slice(-4)}
                    </div>
                  </div>
                </div>
                <GhostButton
                  type="button"
                  onClick={() => updateRemote({ access_token: null, claimed_handle: null })}
                >Unpair</GhostButton>
              </div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                <small style={{ fontSize: 11, color: 'var(--fg-dim)', lineHeight: 1.4 }}>
                  Generate a pairing code on the StarStats website (Hangar → Pair a desktop client) and type it below.
                </small>
                <div style={{ display: 'flex', gap: 8 }}>
                  <TextInput
                    type="text"
                    value={pairingCode}
                    placeholder="ABCDEFGH"
                    maxLength={8}
                    onChange={(e) => setPairingCode(e.target.value.toUpperCase())}
                    style={{
                      flex: 1,
                      letterSpacing: '0.25em', textAlign: 'center',
                      fontWeight: 600, fontSize: 14,
                    }}
                  />
                  <PrimaryButton type="button" disabled={pairingCode.length !== 8}>
                    Pair
                  </PrimaryButton>
                </div>
              </div>
            )}
          </Field>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
            <Field label="Sync interval">
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <TextInput
                  type="number" min={5} max={3600}
                  value={draft.remote_sync.interval_secs}
                  onChange={(e) => updateRemote({ interval_secs: Math.max(5, +e.target.value || 60) })}
                  style={{ flex: 1 }}
                />
                <span style={{ fontSize: 11, color: 'var(--fg-dim)' }}>sec</span>
              </div>
            </Field>
            <Field label="Batch size">
              <TextInput
                type="number" min={1} max={5000}
                value={draft.remote_sync.batch_size}
                onChange={(e) => updateRemote({ batch_size: Math.max(1, +e.target.value || 200) })}
              />
            </Field>
          </div>
        </fieldset>
      </TrayCard>

      <TrayCard
        title="RSI session cookie"
        right={
          <span style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11 }}>
            <StatusDot tone={cookieConfigured ? 'ok' : 'warn'} />
            <span style={{
              color: cookieConfigured ? 'var(--ok)' : 'var(--warn)',
              fontFamily: 'var(--font-mono)', textTransform: 'uppercase', letterSpacing: '0.08em',
            }}>{cookieConfigured ? 'SET' : 'MISSING'}</span>
          </span>
        }
      >
        <p style={{ margin: '0 0 12px', color: 'var(--fg-muted)', fontSize: 12, lineHeight: 1.5 }}>
          {cookieConfigured
            ? <>Configured (last 4 chars: <code style={{ color: 'var(--accent)', fontFamily: 'var(--font-mono)' }}>{cookiePreview}</code>). Paste a new value to rotate.</>
            : 'Not configured. Paste your Rsi-Token cookie below.'}
        </p>

        <Field
          label="Rsi-Token cookie"
          hint="Find this in DevTools → Application → Cookies → robertsspaceindustries.com → Rsi-Token. Never leaves your machine — only parsed ship lists are sent."
        >
          <TextInput
            type="password"
            value={cookieDraft}
            placeholder="•••••••••••••••••••••••••••"
            onChange={(e) => setCookieDraft(e.target.value)}
          />
        </Field>

        <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
          <PrimaryButton type="button" disabled={!cookieDraft.trim()}>Save cookie</PrimaryButton>
          <GhostButton type="button" disabled={!cookieConfigured}>Clear</GhostButton>
        </div>
      </TrayCard>

      <div style={{
        display: 'flex', alignItems: 'center', gap: 12,
        padding: '10px 0',
        borderTop: '1px solid var(--border)',
      }}>
        <PrimaryButton type="button" onClick={() => onSave(draft)}>Save settings</PrimaryButton>
        <span style={{ fontSize: 11, color: 'var(--fg-dim)' }}>
          Changes apply on save. Sync state refetches automatically.
        </span>
      </div>
    </div>
  );
}

// ---------- App -------------------------------------------------
function TrayApp() {
  const [view, setView] = useState('status');
  const [config, setConfig] = useState(MOCK_CONFIG);
  const [authLost, setAuthLost] = useState(false);

  const status = useMemo(() => ({
    ...MOCK_STATUS,
    account: { ...MOCK_STATUS.account, auth_lost: authLost },
  }), [authLost]);

  return (
    <div style={{
      minHeight: '100vh',
      background: 'var(--bg)',
      color: 'var(--fg)',
      fontFamily: 'var(--font-sans)',
      fontSize: 14,
      display: 'flex', flexDirection: 'column',
    }}>
      <TrayHeader view={view} onView={setView} status={status} />
      <main style={{ padding: 14, flex: 1 }}>
        {view === 'status' && (
          <StatusPane
            status={status}
            coverage={MOCK_COVERAGE}
            timeline={MOCK_TIMELINE}
            onGoToSettings={() => setView('settings')}
          />
        )}
        {view === 'logs' && window.LogsPane && <window.LogsPane />}
        {view === 'settings' && (
          <SettingsPane config={config} onSave={setConfig} />
        )}
      </main>
    </div>
  );
}

// Expose primitives for tray-logs.jsx
Object.assign(window, { TrayCard, KV, StatPill, StatusDot, GhostButton, PrimaryButton, TrayApp });
