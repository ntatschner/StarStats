/**
 * Constellation map — distinct stops laid out as nodes grouped by
 * system, with the user's actual path drawn as line segments between
 * chronologically consecutive stops. Recent nodes glow brighter so
 * the eye lands on "where you are now" without scanning labels.
 *
 * Layout strategy (deterministic, server-renderable):
 *   - Each unique system becomes a horizontal "column" of width = totalWidth / systemCount
 *   - Within a column we stack planets vertically; cities sit slightly
 *     offset from their parent planet
 *   - Path edges connect consecutive distinct stops in source order
 *
 * No coordinates exist in the data, so the constellation is symbolic —
 * it groups by hierarchy, not by in-game geography. Names ride the SVG
 * directly so the entire visualization is self-contained and crisp at
 * any zoom level.
 */

import type { TraceEntry } from '@/lib/api';
import { type DistinctStop, toDistinctStops } from './trail-utils';

interface Props {
  entries: TraceEntry[];
  /** Cap node count to keep the SVG readable. Default 30. */
  maxStops?: number;
}

interface PositionedStop extends DistinctStop {
  x: number;
  y: number;
  /** 1.0 = newest, 0.0 = oldest. Drives glow intensity / opacity. */
  recency: number;
  /** Stable order in the input (older→newer). */
  order: number;
}

const VIEW_W = 800;
const VIEW_H = 360;
const PAD_X = 60;
const PAD_TOP = 50;
const PAD_BOTTOM = 30;

export function LocationConstellation({ entries, maxStops = 30 }: Props) {
  const stops = toDistinctStops(entries).slice(-maxStops);
  if (stops.length === 0) {
    return (
      <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
        No stops to chart yet.
      </p>
    );
  }

  const positioned = layout(stops);

  const ordered = [...positioned].sort((a, b) => a.order - b.order);
  const edges: Array<{ a: PositionedStop; b: PositionedStop }> = [];
  for (let i = 1; i < ordered.length; i++) {
    edges.push({ a: ordered[i - 1], b: ordered[i] });
  }

  const systemHeaders = collectSystemHeaders(positioned);

  return (
    <div style={{ width: '100%', overflowX: 'auto' }}>
      <svg
        viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
        role="img"
        aria-label="Constellation of recent in-game stops"
        style={{
          width: '100%',
          maxWidth: '100%',
          minWidth: 480,
          height: 'auto',
          display: 'block',
        }}
      >
        {systemHeaders.map((h, i) => (
          <g key={`hdr-${i}`}>
            <text
              x={h.x}
              y={PAD_TOP - 18}
              textAnchor="middle"
              style={{
                fontSize: 11,
                fill: 'var(--fg-dim)',
                textTransform: 'uppercase',
                letterSpacing: '0.08em',
              }}
            >
              {h.label}
            </text>
            <line
              x1={h.x - 40}
              y1={PAD_TOP - 8}
              x2={h.x + 40}
              y2={PAD_TOP - 8}
              stroke="var(--border)"
              strokeWidth={1}
            />
          </g>
        ))}

        {edges.map((e, i) => {
          const edgeOpacity = 0.25 + 0.55 * Math.min(e.a.recency, e.b.recency);
          return (
            <line
              key={`edge-${i}`}
              x1={e.a.x}
              y1={e.a.y}
              x2={e.b.x}
              y2={e.b.y}
              stroke="var(--accent)"
              strokeWidth={1.2}
              strokeOpacity={edgeOpacity}
              strokeDasharray={i === edges.length - 1 ? '0' : '3 3'}
            />
          );
        })}

        {positioned.map((s) => {
          const radius = 4 + Math.min(6, Math.log2(s.eventCount + 1));
          const glow = s.recency > 0.85;
          return (
            <g key={s.key} transform={`translate(${s.x},${s.y})`}>
              {glow && (
                <circle
                  r={radius + 6}
                  fill="var(--accent)"
                  fillOpacity={0.18}
                />
              )}
              <circle
                r={radius}
                fill={glow ? 'var(--accent)' : 'var(--fg)'}
                fillOpacity={0.4 + 0.6 * s.recency}
                stroke="var(--bg)"
                strokeWidth={1}
              />
              <text
                x={0}
                y={radius + 13}
                textAnchor="middle"
                style={{
                  fontSize: 10,
                  fill: glow ? 'var(--fg)' : 'var(--fg-muted)',
                  fontWeight: glow ? 600 : 400,
                }}
              >
                {truncate(s.label, 14)}
              </text>
              <title>
                {s.label}
                {s.sublabel ? ` · ${s.sublabel}` : ''} — {s.eventCount} event
                {s.eventCount === 1 ? '' : 's'}
              </title>
            </g>
          );
        })}
      </svg>
    </div>
  );
}

function layout(stops: DistinctStop[]): PositionedStop[] {
  const bySystem = new Map<string, DistinctStop[]>();
  const systemOrder: string[] = [];
  for (const s of stops) {
    const sys = s.system ?? '—';
    if (!bySystem.has(sys)) {
      bySystem.set(sys, []);
      systemOrder.push(sys);
    }
    bySystem.get(sys)!.push(s);
  }

  const colWidth =
    systemOrder.length === 1
      ? 0
      : (VIEW_W - 2 * PAD_X) / (systemOrder.length - 1);
  const trackHeight = VIEW_H - PAD_TOP - PAD_BOTTOM;

  const out: PositionedStop[] = [];
  const stopsByKey = new Map<string, number>();
  stops.forEach((s, i) => stopsByKey.set(s.key, i));

  systemOrder.forEach((sys, sysIdx) => {
    const colX =
      systemOrder.length === 1 ? VIEW_W / 2 : PAD_X + sysIdx * colWidth;
    const within = bySystem.get(sys)!;
    within.forEach((s, i) => {
      const stepY =
        within.length === 1
          ? trackHeight / 2
          : (i / (within.length - 1)) * trackHeight;
      const horizontalJitter = s.city ? 18 : 0;
      out.push({
        ...s,
        x: colX + horizontalJitter,
        y: PAD_TOP + stepY,
        recency:
          stops.length === 1
            ? 1
            : (stopsByKey.get(s.key) ?? 0) / (stops.length - 1),
        order: stopsByKey.get(s.key) ?? 0,
      });
    });
  });

  return out;
}

function collectSystemHeaders(
  positioned: PositionedStop[],
): Array<{ x: number; label: string }> {
  const seen = new Map<string, { sumX: number; count: number }>();
  for (const p of positioned) {
    const label = p.system ?? '—';
    const entry = seen.get(label) ?? { sumX: 0, count: 0 };
    entry.sumX += p.x;
    entry.count += 1;
    seen.set(label, entry);
  }
  return Array.from(seen.entries()).map(([label, { sumX, count }]) => ({
    label,
    x: sumX / count,
  }));
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max - 1) + '…';
}
