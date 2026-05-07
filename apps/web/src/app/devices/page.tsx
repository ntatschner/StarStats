import { redirect } from 'next/navigation';
import {
  ApiCallError,
  listDevices,
  revokeDevice,
  startPairing,
} from '@/lib/api';
import { getSession } from '@/lib/session';

interface SearchParams {
  code?: string;
  expires?: string;
  error?: string;
}

const mainStyle: React.CSSProperties = {
  maxWidth: 'none',
  margin: 0,
  padding: 0,
  display: 'flex',
  flexDirection: 'column',
  gap: 20,
};

const headerTitleStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 32,
  fontWeight: 600,
  letterSpacing: '-0.02em',
};

const headerSubtitleStyle: React.CSSProperties = {
  margin: '6px 0 0',
  color: 'var(--fg-muted)',
  fontSize: 14,
};

const stepGridStyle: React.CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
  gap: 16,
};

const cardHeaderStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 4,
  marginBottom: 16,
};

const cardTitleStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 18,
  fontWeight: 600,
  letterSpacing: '-0.01em',
};

const formStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 14,
  margin: 0,
};

const formActionsStyle: React.CSSProperties = {
  display: 'flex',
  marginTop: 4,
};

const pairCodeFrameStyle: React.CSSProperties = {
  background: 'var(--bg-elev)',
  border: '1px dashed var(--accent)',
  borderRadius: 'var(--r-md)',
  padding: '26px 24px',
  textAlign: 'center',
};

const pairCodeStyle: React.CSSProperties = {
  fontSize: 38,
  color: 'var(--accent)',
  letterSpacing: '0.18em',
  fontWeight: 500,
  userSelect: 'all',
};

const pairCodeMetaStyle: React.CSSProperties = {
  marginTop: 12,
  color: 'var(--fg-dim)',
  fontSize: 12,
};

const pairCodeFootnoteStyle: React.CSSProperties = {
  marginTop: 12,
  color: 'var(--fg-dim)',
  fontSize: 12,
};

const tableLabelCellStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
};

const emptyStyle: React.CSSProperties = {
  textAlign: 'center',
  padding: '40px 20px',
  color: 'var(--fg-muted)',
  fontSize: 14,
};

const emptyTitleStyle: React.CSSProperties = {
  fontSize: 16,
  color: 'var(--fg)',
  marginBottom: 6,
};

const sessionFootStyle: React.CSSProperties = {
  marginTop: 12,
  color: 'var(--fg-muted)',
  fontSize: 13,
};

export default async function DevicesPage(props: {
  searchParams: Promise<SearchParams>;
}) {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/devices');

  const { code, expires, error } = await props.searchParams;

  // Pull the current device list. If the token has expired (401),
  // bounce to login rather than rendering a half-broken page.
  let devices: Awaited<ReturnType<typeof listDevices>> = { devices: [] };
  try {
    devices = await listDevices(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/devices');
    }
    throw e;
  }

  async function pairAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/devices');

    const label = String(formData.get('label') ?? '').trim();
    try {
      const pairing = await startPairing(session.token, { label });
      const params = new URLSearchParams({
        code: pairing.code,
        expires: pairing.expires_at,
      });
      redirect(`/devices?${params.toString()}`);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        redirect('/auth/login?next=/devices');
      }
      throw e;
    }
  }

  async function revokeAction(formData: FormData) {
    'use server';
    const session = await getSession();
    if (!session) redirect('/auth/login?next=/devices');

    const id = String(formData.get('device_id') ?? '');
    if (!id) redirect('/devices?error=missing_id');

    try {
      await revokeDevice(session.token, id);
    } catch (e) {
      if (e instanceof ApiCallError && e.status === 401) {
        redirect('/auth/login?next=/devices');
      }
      throw e;
    }
    redirect('/devices');
  }

  const pairedCount = devices.devices.length;

  return (
    <main className="ss-screen-enter" style={mainStyle}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Hangar · paired clients
        </div>
        <h1 style={headerTitleStyle}>Pair a desktop client</h1>
        <p style={headerSubtitleStyle}>
          Run the StarStats tray app, click <em>Pair</em>, type the code below.
          Codes expire in 5 minutes and burn on first use.
        </p>
      </header>

      {error && (
        <div className="ss-alert ss-alert--danger" role="alert">
          Couldn&apos;t complete that action. Try again.
        </div>
      )}

      <div style={stepGridStyle}>
        <section className="ss-card ss-card-pad">
          <div style={cardHeaderStyle}>
            <span className="ss-eyebrow">Step 1</span>
            <h2 style={cardTitleStyle}>Generate a pairing code</h2>
          </div>
          <form action={pairAction} style={formStyle}>
            <label className="ss-label">
              <span className="ss-label-text">Device label</span>
              <input
                className="ss-input"
                type="text"
                name="label"
                placeholder="Daisy's gaming PC"
                spellCheck={false}
                autoComplete="off"
              />
              <small style={{ color: 'var(--fg-dim)', fontSize: 12 }}>
                Optional — helps you tell devices apart in the Hangar list.
              </small>
            </label>
            <div style={formActionsStyle}>
              <button type="submit" className="ss-btn ss-btn--primary">
                Generate pairing code
              </button>
            </div>
          </form>
        </section>

        <section className="ss-card ss-card-pad">
          <div style={cardHeaderStyle}>
            <span className="ss-eyebrow">
              {code ? 'Active code' : 'Awaiting code'}
            </span>
            <h2 style={cardTitleStyle}>Paste this into the tray app</h2>
          </div>
          {code ? (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
              <div style={pairCodeFrameStyle}>
                <div className="mono ss-pair-code" style={pairCodeStyle}>
                  {code}
                </div>
                {expires && (
                  <div style={pairCodeMetaStyle}>
                    Expires{' '}
                    <span
                      className="mono"
                      style={{ color: 'var(--fg-muted)' }}
                    >
                      <ExpiryRelative iso={expires} />
                    </span>
                  </div>
                )}
              </div>
              <div style={pairCodeFootnoteStyle}>
                Each code is single-use. Generate a new one if it expires.
              </div>
            </div>
          ) : (
            <div
              style={{
                ...pairCodeFrameStyle,
                borderStyle: 'dashed',
                borderColor: 'var(--border-strong)',
              }}
            >
              <div
                className="mono ss-pair-code"
                style={{ ...pairCodeStyle, color: 'var(--fg-dim)' }}
              >
                ———
              </div>
              <div style={pairCodeMetaStyle}>
                Generate a code on the left to begin.
              </div>
            </div>
          )}
        </section>
      </div>

      <section className="ss-card ss-card-pad">
        <div style={cardHeaderStyle}>
          <span className="ss-eyebrow">Manifest</span>
          <h2 style={cardTitleStyle}>
            Paired devices ({pairedCount})
          </h2>
        </div>
        {pairedCount === 0 ? (
          <div style={emptyStyle}>
            <div style={emptyTitleStyle}>Scope is clear.</div>
            <div>
              No devices yet. Generate a code above and pair the tray app.
            </div>
          </div>
        ) : (
          <div className="ss-table-wrap">
            <table className="ss-table">
              <thead>
                <tr>
                  <th>Label</th>
                  <th>Paired</th>
                  <th>Last seen</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {devices.devices.map((d) => {
                  const isOnline =
                    !!d.last_seen_at &&
                    Date.now() - Date.parse(d.last_seen_at) < 5 * 60 * 1000;
                  return (
                    <tr key={d.id}>
                      <td>
                        <div style={tableLabelCellStyle}>
                          <span className="mono">{d.label}</span>
                          {isOnline && (
                            <span className="ss-badge ss-badge--ok">
                              <span className="ss-badge-dot" />
                              Online
                            </span>
                          )}
                        </div>
                      </td>
                      <td style={{ color: 'var(--fg-muted)' }}>
                        <RelativeTime iso={d.created_at} />
                      </td>
                      <td style={{ color: 'var(--fg-muted)' }}>
                        {d.last_seen_at ? (
                          <RelativeTime iso={d.last_seen_at} />
                        ) : (
                          <span style={{ color: 'var(--fg-dim)' }}>never</span>
                        )}
                      </td>
                      <td className="ss-num">
                        <form
                          action={revokeAction}
                          style={{ display: 'inline' }}
                        >
                          <input
                            type="hidden"
                            name="device_id"
                            value={d.id}
                          />
                          <button
                            type="submit"
                            className="ss-btn ss-btn--danger"
                            style={{ padding: '6px 12px', fontSize: 12 }}
                          >
                            Revoke
                          </button>
                        </form>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <p style={sessionFootStyle}>
        Signed in as <strong>{session.claimedHandle}</strong>.
      </p>
    </main>
  );
}

function ExpiryRelative({ iso }: { iso: string }) {
  const seconds = Math.max(
    0,
    Math.round((Date.parse(iso) - Date.now()) / 1000),
  );
  if (seconds < 60) return <>in {seconds}s</>;
  return <>in ~{Math.round(seconds / 60)}m</>;
}

function RelativeTime({ iso }: { iso: string }) {
  const seconds = Math.max(
    0,
    Math.round((Date.now() - Date.parse(iso)) / 1000),
  );
  if (seconds < 60) return <>just now</>;
  if (seconds < 3600) return <>{Math.round(seconds / 60)}m ago</>;
  if (seconds < 86400) return <>{Math.round(seconds / 3600)}h ago</>;
  return <>{Math.round(seconds / 86400)}d ago</>;
}
