/**
 * SparklinePill — a stat pill backed by a tiny inline-SVG sparkline.
 *
 * Renders the headline number (e.g. "23 kills") above a trend line.
 * Skips the full `MetricCard` shell because pills are small and
 * dense — they live in a strip of 3-5 pills, not as standalone cards.
 */

'use client';

import { isFlagEnabled } from '@/lib/feature-flags';

export interface SparklinePillProps {
  value: string;
  label: string;
  series: number[];
  caption?: string;
}

const W = 96;
const H = 28;

function buildPath(series: number[]): string {
  if (series.length === 0) return '';
  const max = Math.max(...series, 1);
  const min = Math.min(...series, 0);
  const range = Math.max(max - min, 1);
  const stepX = W / Math.max(series.length - 1, 1);
  return series
    .map((v, i) => {
      const x = i * stepX;
      const y = H - ((v - min) / range) * H;
      return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');
}

function buildArea(series: number[]): string {
  const line = buildPath(series);
  if (line.length === 0) return '';
  return `${line} L${W.toFixed(1)},${H.toFixed(1)} L0,${H.toFixed(1)} Z`;
}

export function SparklinePill(props: SparklinePillProps) {
  const { value, label, series, caption } = props;
  if (!isFlagEnabled('metrics.sparkline_pills')) return null;

  const hasData = series.length > 0 && series.some((v) => v > 0);

  return (
    <div className="ss-stat sparkline-pill" role="group" aria-label={`${value} ${label}`}>
      <div className="sparkline-pill__head">
        <div className="sparkline-pill__value">{value}</div>
        <div className="sparkline-pill__label">{label}</div>
      </div>
      {hasData ? (
        <svg
          className="sparkline-pill__svg"
          width={W}
          height={H}
          viewBox={`0 0 ${W} ${H}`}
          role="img"
          aria-label={`${label} trend over ${series.length} points`}
        >
          <path d={buildArea(series)} fill="var(--accent-soft)" stroke="none" />
          <path
            d={buildPath(series)}
            fill="none"
            stroke="var(--accent)"
            strokeWidth={1.5}
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      ) : (
        <div className="sparkline-pill__svg sparkline-pill__svg--empty" aria-hidden="true" />
      )}
      {caption ? <div className="sparkline-pill__caption">{caption}</div> : null}
    </div>
  );
}
