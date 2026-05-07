/**
 * Tray-specific primitives mirroring the design package's
 * `tray-app.jsx` atoms. Compact density variants of the web's `ss-*`
 * primitives — tray runs at ~720px, so paddings and font sizes are
 * tighter than the equivalent web-app cards.
 *
 * Consumers: StatusPane, SettingsPane, future LogsPane.
 */

import { type ReactNode, type CSSProperties, type InputHTMLAttributes, type ButtonHTMLAttributes } from 'react';

export type Tone = 'ok' | 'warn' | 'danger' | 'accent' | 'info' | 'dim' | 'default';

interface TrayCardProps {
  title?: ReactNode;
  kicker?: ReactNode;
  right?: ReactNode;
  children: ReactNode;
  mono?: boolean;
}

export function TrayCard({ title, kicker, right, children, mono = false }: TrayCardProps) {
  return (
    <section
      style={{
        background: 'var(--surface)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--r-md)',
        padding: '14px 16px',
      }}
    >
      {(title || right) && (
        <header
          style={{
            display: 'flex',
            alignItems: 'baseline',
            justifyContent: 'space-between',
            gap: 12,
            marginBottom: 10,
          }}
        >
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
            {title && (
              <h2
                style={{
                  margin: 0,
                  fontSize: 11,
                  fontWeight: 600,
                  color: 'var(--fg-muted)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.12em',
                  fontFamily: mono ? 'var(--font-mono)' : 'var(--font-sans)',
                }}
              >
                {title}
              </h2>
            )}
            {kicker && (
              <span
                style={{
                  fontSize: 11,
                  color: 'var(--fg-dim)',
                  fontFamily: 'var(--font-mono)',
                }}
              >
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

interface KVProps {
  label: ReactNode;
  value: ReactNode;
  mono?: boolean;
  dim?: boolean;
}

export function KV({ label, value, mono = false, dim = false }: KVProps) {
  return (
    <>
      <dt style={{ color: 'var(--fg-muted)', fontSize: 12 }}>{label}</dt>
      <dd
        style={{
          margin: 0,
          fontVariantNumeric: 'tabular-nums',
          fontSize: 13,
          color: dim ? 'var(--fg-dim)' : 'var(--fg)',
          fontFamily: mono ? 'var(--font-mono)' : 'var(--font-sans)',
          wordBreak: mono ? 'break-all' : 'normal',
        }}
      >
        {value}
      </dd>
    </>
  );
}

interface StatPillProps {
  label: ReactNode;
  value: ReactNode;
  tone?: Tone;
}

const PILL_TONES: Record<Tone, string> = {
  default: 'var(--fg)',
  ok: 'var(--ok)',
  warn: 'var(--warn)',
  danger: 'var(--danger)',
  accent: 'var(--accent)',
  info: 'var(--info)',
  dim: 'var(--fg-dim)',
};

export function StatPill({ label, value, tone = 'default' }: StatPillProps) {
  return (
    <div
      style={{
        flex: '1 1 0',
        minWidth: 0,
        background: 'var(--surface-2)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--r-sm)',
        padding: '8px 10px',
      }}
    >
      <div
        style={{
          fontSize: 10,
          color: 'var(--fg-muted)',
          textTransform: 'uppercase',
          letterSpacing: '0.1em',
          marginBottom: 3,
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontFamily: 'var(--font-mono)',
          fontSize: 16,
          fontWeight: 600,
          color: PILL_TONES[tone],
          fontVariantNumeric: 'tabular-nums',
        }}
      >
        {value}
      </div>
    </div>
  );
}

interface StatusDotProps {
  tone?: Tone;
}

const DOT_TONES: Record<Tone, string> = {
  ok: 'var(--ok)',
  warn: 'var(--warn)',
  danger: 'var(--danger)',
  accent: 'var(--accent)',
  info: 'var(--info)',
  dim: 'var(--fg-dim)',
  default: 'var(--fg-muted)',
};

export function StatusDot({ tone = 'ok' }: StatusDotProps) {
  const colour = DOT_TONES[tone];
  return (
    <span
      style={{
        display: 'inline-block',
        width: 8,
        height: 8,
        borderRadius: '50%',
        background: colour,
        boxShadow: `0 0 0 3px ${colour}22`,
        flexShrink: 0,
      }}
    />
  );
}

interface BannerProps {
  tone?: 'warn' | 'info' | 'danger';
  children: ReactNode;
  action?: string;
  onAction?: () => void;
}

const BANNER_TONES: Record<'warn' | 'info' | 'danger', { border: string; bg: string; fg: string }> = {
  warn: { border: 'var(--warn)', bg: 'rgba(232, 197, 60, 0.08)', fg: 'var(--warn)' },
  info: { border: 'var(--info)', bg: 'rgba(111, 168, 232, 0.08)', fg: 'var(--info)' },
  danger: { border: 'var(--danger)', bg: 'rgba(232, 103, 76, 0.08)', fg: 'var(--danger)' },
};

export function Banner({ tone = 'info', children, action, onAction }: BannerProps) {
  const t = BANNER_TONES[tone];
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 12,
        padding: '10px 14px',
        borderRadius: 'var(--r-sm)',
        border: `1px solid ${t.border}`,
        background: t.bg,
        color: t.fg,
        fontSize: 13,
      }}
      role="status"
    >
      <span>{children}</span>
      {action && (
        <button
          type="button"
          onClick={onAction}
          style={{
            background: 'transparent',
            color: 'inherit',
            border: '1px solid currentColor',
            borderRadius: 'var(--r-sm)',
            padding: '4px 10px',
            fontWeight: 600,
            fontSize: 12,
            cursor: 'pointer',
            whiteSpace: 'nowrap',
            fontFamily: 'inherit',
          }}
        >
          {action}
        </button>
      )}
    </div>
  );
}

interface FieldProps {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
}

export function Field({ label, hint, children }: FieldProps) {
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <span
        style={{
          fontSize: 10,
          fontWeight: 600,
          color: 'var(--fg-muted)',
          textTransform: 'uppercase',
          letterSpacing: '0.1em',
        }}
      >
        {label}
      </span>
      {children}
      {hint && (
        <small style={{ fontSize: 11, color: 'var(--fg-dim)', lineHeight: 1.4 }}>{hint}</small>
      )}
    </label>
  );
}

const INPUT_BASE: CSSProperties = {
  background: 'var(--bg)',
  color: 'var(--fg)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--r-sm)',
  padding: '7px 9px',
  fontFamily: 'var(--font-mono)',
  fontSize: 12,
  outline: 'none',
};

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  const { style, ...rest } = props;
  return <input {...rest} style={{ ...INPUT_BASE, ...(style ?? {}) }} />;
}

export function PrimaryButton({
  children,
  style,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      style={{
        background: 'var(--accent)',
        color: 'var(--accent-fg)',
        border: 'none',
        borderRadius: 'var(--r-sm)',
        padding: '7px 14px',
        fontWeight: 600,
        fontSize: 12,
        cursor: props.disabled ? 'not-allowed' : 'pointer',
        opacity: props.disabled ? 0.55 : 1,
        fontFamily: 'inherit',
        letterSpacing: '0.02em',
        ...(style ?? {}),
      }}
    >
      {children}
    </button>
  );
}

export function GhostButton({
  children,
  style,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      style={{
        background: 'transparent',
        color: 'var(--fg-muted)',
        border: '1px solid var(--border-strong)',
        borderRadius: 'var(--r-sm)',
        padding: '6px 12px',
        fontWeight: 500,
        fontSize: 12,
        cursor: props.disabled ? 'not-allowed' : 'pointer',
        opacity: props.disabled ? 0.55 : 1,
        fontFamily: 'inherit',
        ...(style ?? {}),
      }}
    >
      {children}
    </button>
  );
}
