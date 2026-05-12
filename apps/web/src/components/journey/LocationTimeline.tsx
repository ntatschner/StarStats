/**
 * Vertical timeline of distinct stops with a left ribbon, dwell badges,
 * and event-count chips. Replaces the inline ordered-list rendering on
 * the /journey Location tab — same data, more visual hierarchy.
 *
 * Server component. Takes pre-fetched `TraceEntry[]` via props. Dwell
 * time per stop is derived from (enteredAt, lastSeenAt) within the run
 * — not perfect (the run ends when the user moved, not when we last
 * polled), but accurate to the gap-event granularity the server emits.
 */

import type { TraceEntry } from '@/lib/api';
import {
  type DistinctStop,
  toDistinctStops,
  glyphFor,
  formatDwell,
  relativeAge,
} from './trail-utils';

interface Props {
  entries: TraceEntry[];
  /** Cap to avoid runaway lists on very active sessions. */
  maxStops?: number;
}

export function LocationTimeline({ entries, maxStops = 40 }: Props) {
  // Newest at top reads better on /journey — flip the oldest→newest
  // order from `toDistinctStops`.
  const stops = toDistinctStops(entries).slice(-maxStops).reverse();

  if (stops.length === 0) {
    return (
      <p style={{ margin: 0, color: 'var(--fg-dim)', fontSize: 13 }}>
        No location-bearing events in the window.
      </p>
    );
  }

  return (
    <ol
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        position: 'relative',
        display: 'flex',
        flexDirection: 'column',
        gap: 0,
      }}
    >
      <span
        aria-hidden
        style={{
          position: 'absolute',
          left: 9,
          top: 14,
          bottom: 14,
          width: 1,
          background: 'var(--border)',
        }}
      />
      {stops.map((stop, i) => (
        <TimelineRow
          key={stop.key + stop.enteredAt}
          stop={stop}
          isLatest={i === 0}
        />
      ))}
    </ol>
  );
}

function TimelineRow({
  stop,
  isLatest,
}: {
  stop: DistinctStop;
  isLatest: boolean;
}) {
  const dwellSec = dwellSeconds(stop);
  return (
    <li
      style={{
        position: 'relative',
        display: 'grid',
        gridTemplateColumns: '20px 1fr auto',
        gap: 12,
        alignItems: 'baseline',
        padding: '10px 0',
      }}
    >
      <span
        aria-hidden
        style={{
          display: 'inline-block',
          width: 10,
          height: 10,
          marginTop: 6,
          borderRadius: '50%',
          background: isLatest ? 'var(--accent)' : 'var(--fg-dim)',
          boxShadow: '0 0 0 3px var(--bg)',
          marginLeft: 4,
        }}
      />
      <div style={{ minWidth: 0 }}>
        <div
          style={{
            display: 'flex',
            alignItems: 'baseline',
            gap: 8,
            fontSize: 14,
            color: 'var(--fg)',
          }}
        >
          <span aria-hidden style={{ fontSize: 13, opacity: 0.85 }}>
            {glyphFor(stop)}
          </span>
          <strong style={{ fontWeight: isLatest ? 600 : 500 }}>
            {stop.label}
          </strong>
          {stop.sublabel && (
            <span style={{ color: 'var(--fg-muted)', fontSize: 12 }}>
              · {stop.sublabel}
            </span>
          )}
        </div>
        <div
          style={{
            marginTop: 2,
            fontSize: 11,
            color: 'var(--fg-dim)',
            display: 'flex',
            gap: 10,
            flexWrap: 'wrap',
          }}
        >
          <span className="mono" title={stop.enteredAt}>
            entered {relativeAge(stop.enteredAt)} ago
          </span>
          {dwellSec > 0 && (
            <span
              className="mono"
              title="time between first and last event at this stop"
            >
              · dwell {formatDwell(dwellSec)}
            </span>
          )}
          <span className="mono">
            · {stop.eventCount} event{stop.eventCount === 1 ? '' : 's'}
          </span>
        </div>
      </div>
    </li>
  );
}

function dwellSeconds(stop: DistinctStop): number {
  const a = new Date(stop.enteredAt).getTime();
  const b = new Date(stop.lastSeenAt).getTime();
  if (Number.isNaN(a) || Number.isNaN(b) || b <= a) return 0;
  return Math.round((b - a) / 1000);
}
