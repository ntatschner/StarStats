/**
 * Display-formatting helpers shared across tray panes (Status, Logs).
 * Each function is pure and locale-aware where it makes sense; callers
 * should not need to think about NaN, negative, or missing values.
 */

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

export function fmtTime(iso: string | null | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleTimeString();
}

export function fmtDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

export function ageLabel(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  if (Number.isNaN(ms)) return iso;
  if (ms < 60_000) return `${Math.floor(ms / 1000)}s ago`;
  if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ago`;
  if (ms < 86_400_000) return `${Math.floor(ms / 3_600_000)}h ago`;
  return `${Math.floor(ms / 86_400_000)}d ago`;
}

export function fmtCovPct(recognised: number, structuralOnly: number): string {
  const total = recognised + structuralOnly;
  return total === 0 ? '—' : `${((recognised / total) * 100).toFixed(1)}%`;
}

export type RowTone = 'ok' | 'warn' | 'danger' | 'accent' | 'info';

export const TONE_VAR: Record<RowTone, string> = {
  ok: 'var(--ok)',
  warn: 'var(--warn)',
  danger: 'var(--danger)',
  accent: 'var(--accent)',
  info: 'var(--info)',
};

/**
 * Maps an event_type to a tone. Shared by StatusPane's timeline and
 * LogsPane's grouped list so the same event paints the same colour
 * across panes.
 */
export function toneForType(eventType: string): RowTone {
  switch (eventType) {
    case 'actor_death':
    case 'vehicle_destruction':
      return 'danger';
    case 'legacy_login':
    case 'join_pu':
    case 'mission_completed':
      return 'ok';
    case 'quantum_target_selected':
      return 'accent';
    default:
      return 'info';
  }
}
