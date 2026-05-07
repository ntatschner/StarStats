/**
 * Tray header — mirrors `tray-app.jsx`'s `TrayHeader`. 3-col grid:
 * brand mark + version on the left, view tabs in the centre, tailing
 * status pill on the right.
 */

import { StatusDot } from './tray/primitives';

export type TrayView = 'status' | 'logs' | 'settings';

interface Props {
  view: TrayView;
  onView: (next: TrayView) => void;
  isTailing: boolean;
  version: string;
}

const TABS: ReadonlyArray<TrayView> = ['status', 'logs', 'settings'];

export function TrayHeader({ view, onView, isTailing, version }: Props) {
  return (
    <header
      style={{
        display: 'grid',
        gridTemplateColumns: 'auto 1fr auto',
        alignItems: 'center',
        gap: 16,
        padding: '12px 16px',
        borderBottom: '1px solid var(--border)',
        background: 'var(--bg-elev)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <span
          style={{
            fontFamily: 'var(--font-mono)',
            fontSize: 16,
            color: 'var(--accent)',
            fontWeight: 700,
            letterSpacing: '-0.02em',
          }}
          aria-hidden="true"
        >
          ★
        </span>
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <div
            style={{
              fontWeight: 700,
              fontSize: 13,
              letterSpacing: '0.06em',
              textTransform: 'uppercase',
            }}
          >
            STARSTATS
          </div>
          <div style={{ fontSize: 10, color: 'var(--fg-dim)', letterSpacing: '0.04em' }}>
            Tray client · v{version}
          </div>
        </div>
      </div>

      <nav style={{ display: 'flex', gap: 4, justifyContent: 'center' }} aria-label="Pane">
        {TABS.map((tab) => {
          const active = view === tab;
          return (
            <button
              key={tab}
              type="button"
              onClick={() => onView(tab)}
              style={{
                background: active ? 'var(--accent-soft)' : 'transparent',
                color: active ? 'var(--accent)' : 'var(--fg-muted)',
                border: `1px solid ${active ? 'var(--accent)' : 'transparent'}`,
                borderRadius: 'var(--r-sm)',
                padding: '5px 14px',
                fontFamily: 'inherit',
                fontSize: 12,
                fontWeight: 600,
                textTransform: 'uppercase',
                letterSpacing: '0.08em',
                cursor: 'pointer',
              }}
            >
              {tab}
            </button>
          );
        })}
      </nav>

      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          fontSize: 11,
          color: 'var(--fg-muted)',
        }}
      >
        <StatusDot tone={isTailing ? 'ok' : 'dim'} />
        <span style={{ fontFamily: 'var(--font-mono)' }}>
          {isTailing ? 'TAILING' : 'IDLE'}
        </span>
      </div>
    </header>
  );
}
