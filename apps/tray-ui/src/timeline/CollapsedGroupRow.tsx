/**
 * One row in the timeline — represents either a single envelope
 * (`row.count === 1`) or a folded run of adjacent same-key envelopes
 * (`row.count > 1`). The summary line is drawn from `row.anchor`;
 * clicking the count badge drills in to reveal the full member list.
 *
 * `InferredBadge` renders next to the summary when the anchor's
 * `metadata.source === 'inferred'`. Phase 3 will surface per-field
 * provenance + the source-event trail here; for now we only show the
 * "inferred + confidence" pill.
 */

import { useState } from 'react';
import type { TimelineRow } from './grouping';
import type { EventEnvelope } from 'api-client-ts';
import { InferredBadge } from './InferredBadge';

interface Props {
  row: TimelineRow;
}

function eventTypeOf(ev: EventEnvelope): string {
  // The wire `event` payload is internally tagged on `type`. The
  // generated type widens it to `Record<string, never>` because
  // openapi-typescript can't represent the full `GameEvent` tagged
  // union; reading the discriminator off through `unknown` is the
  // honest cast.
  const tagged = ev.event as unknown as { type?: string } | null;
  return tagged?.type ?? 'unknown_event';
}

function eventTimestampOf(ev: EventEnvelope): string {
  const tagged = ev.event as unknown as { timestamp?: string } | null;
  return tagged?.timestamp ?? '';
}

export function CollapsedGroupRow({ row }: Props) {
  const [expanded, setExpanded] = useState(false);
  const { anchor, count, members } = row;
  const isFolded = count > 1;
  const inferred = anchor.metadata?.source === 'inferred';
  const confidence = anchor.metadata?.confidence ?? 1;

  const summary = eventTypeOf(anchor);
  const timestamp = eventTimestampOf(anchor);

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        padding: '6px 8px',
        background: 'var(--surface-2)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--r-sm)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 10,
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            minWidth: 0,
          }}
        >
          <span
            style={{
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              color: 'var(--fg)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {summary}
          </span>
          {inferred && <InferredBadge confidence={confidence} />}
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {timestamp && (
            <span
              style={{
                fontFamily: 'var(--font-mono)',
                fontSize: 11,
                color: 'var(--fg-dim)',
                whiteSpace: 'nowrap',
              }}
            >
              {timestamp}
            </span>
          )}
          {isFolded && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              aria-expanded={expanded}
              style={{
                background: 'var(--surface-1)',
                color: 'var(--fg-muted)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--r-pill)',
                padding: '1px 8px',
                fontSize: 11,
                fontFamily: 'var(--font-mono)',
                fontVariantNumeric: 'tabular-nums',
                cursor: 'pointer',
              }}
            >
              {`×${count}`}
            </button>
          )}
        </div>
      </div>
      {expanded && isFolded && (
        <ul
          data-testid="group-row-members"
          style={{
            listStyle: 'none',
            margin: 0,
            padding: 0,
            display: 'flex',
            flexDirection: 'column',
            gap: 2,
            borderTop: '1px solid var(--border)',
            paddingTop: 6,
            marginTop: 2,
          }}
        >
          {members.map((m) => (
            <li
              key={m.idempotency_key}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                gap: 8,
                fontFamily: 'var(--font-mono)',
                fontSize: 11,
                color: 'var(--fg-dim)',
              }}
            >
              <span
                style={{
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {eventTypeOf(m)}
              </span>
              <span style={{ whiteSpace: 'nowrap' }}>
                {eventTimestampOf(m)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
