/**
 * Pill rendered next to events whose `metadata.source === 'inferred'`.
 * Phase 3 will flesh this out with tooltips listing the source events
 * + rule id; for now it stays a one-line visual cue so the timeline
 * can already distinguish observed from inferred rows.
 */

interface Props {
  confidence: number;
}

function clampConfidence(value: number): number {
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

export function InferredBadge({ confidence }: Props) {
  const pct = Math.round(clampConfidence(confidence) * 100);
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        padding: '2px 8px',
        borderRadius: 'var(--r-pill)',
        background: 'var(--surface-2)',
        border: '1px solid var(--border)',
        color: 'var(--fg-muted)',
        fontSize: 10,
        fontWeight: 600,
        textTransform: 'uppercase',
        letterSpacing: '0.08em',
        fontFamily: 'var(--font-sans)',
      }}
      title={`Inferred event (confidence ${pct}%)`}
    >
      <span>Inferred</span>
      <span
        style={{
          fontFamily: 'var(--font-mono)',
          color: 'var(--fg-dim)',
          letterSpacing: 0,
        }}
      >
        {pct}%
      </span>
    </span>
  );
}
