/**
 * One bucket in the by-entity timeline view. Renders a header
 * (entity name + event count) and a list of `CollapsedGroupRow`s
 * derived from `section.rows`.
 *
 * Header click toggles the section open / closed. Sections default
 * to expanded so the user lands on a usable view without having to
 * fan everything out; long-lived noisy entities can be collapsed by
 * the user as needed.
 */

import { useState } from 'react';
import type { EntitySection as EntitySectionData } from './grouping';
import { CollapsedGroupRow } from './CollapsedGroupRow';

interface Props {
  section: EntitySectionData;
  /** Defaults to true — sections render open. Tests use this to
   *  exercise the collapsed branch without having to drive the
   *  toggle from a real user-event. */
  initialExpanded?: boolean;
}

export function EntitySection({ section, initialExpanded = true }: Props) {
  const [expanded, setExpanded] = useState(initialExpanded);
  const count = section.events.length;
  const label = count === 1 ? '1 event' : `${count} events`;

  return (
    <section
      className="ss-card"
      style={{
        padding: '10px 12px',
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
      }}
    >
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
        style={{
          display: 'flex',
          alignItems: 'baseline',
          justifyContent: 'space-between',
          gap: 12,
          width: '100%',
          background: 'transparent',
          border: 'none',
          padding: 0,
          textAlign: 'left',
          cursor: 'pointer',
          color: 'var(--fg)',
          fontFamily: 'inherit',
        }}
      >
        <span
          style={{
            display: 'flex',
            alignItems: 'baseline',
            gap: 8,
            minWidth: 0,
          }}
        >
          <span
            style={{
              fontSize: 10,
              fontWeight: 600,
              color: 'var(--fg-muted)',
              textTransform: 'uppercase',
              letterSpacing: '0.12em',
            }}
          >
            {section.entity.kind}
          </span>
          <span
            style={{
              fontSize: 13,
              fontWeight: 600,
              color: 'var(--fg)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {section.entity.display_name}
          </span>
        </span>
        <span
          style={{
            fontSize: 11,
            color: 'var(--fg-dim)',
            fontFamily: 'var(--font-mono)',
            whiteSpace: 'nowrap',
          }}
        >
          {label}
        </span>
      </button>
      {expanded && (
        <ul
          style={{
            listStyle: 'none',
            margin: 0,
            padding: 0,
            display: 'flex',
            flexDirection: 'column',
            gap: 4,
          }}
        >
          {section.rows.map((row) => (
            <li key={row.anchor.idempotency_key}>
              <CollapsedGroupRow row={row} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
