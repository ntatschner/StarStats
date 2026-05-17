/**
 * Top-level Timeline component. Renders the user's event stream in
 * one of two modes:
 *   - `by-entity` (default): events bucketed by primary entity, each
 *     bucket rendered as an `EntitySection`.
 *   - `chronological`: a flat list with adjacent same-key rows folded
 *     into `CollapsedGroupRow`s.
 *
 * The choice is persisted in localStorage under
 * `tray.timeline.view` so the user's preference survives reloads.
 */

import { useEffect, useState } from 'react';
import type { EventEnvelope } from 'api-client-ts';
import { groupEventsForTimeline, foldAdjacentSameKey } from './grouping';
import { EntitySection } from './EntitySection';
import { CollapsedGroupRow } from './CollapsedGroupRow';

export const TIMELINE_VIEW_STORAGE_KEY = 'tray.timeline.view';

export type TimelineView = 'by-entity' | 'chronological';

const DEFAULT_VIEW: TimelineView = 'by-entity';

function readStoredView(): TimelineView {
  try {
    const raw = window.localStorage.getItem(TIMELINE_VIEW_STORAGE_KEY);
    if (raw === 'by-entity' || raw === 'chronological') return raw;
  } catch {
    // localStorage can throw in private-mode browsers or when the
    // storage quota is exhausted; fall through to the default.
  }
  return DEFAULT_VIEW;
}

function writeStoredView(view: TimelineView): void {
  try {
    window.localStorage.setItem(TIMELINE_VIEW_STORAGE_KEY, view);
  } catch {
    // Silently swallow — losing persistence is a UX regression but
    // not a functional failure.
  }
}

interface Props {
  events: EventEnvelope[];
}

export function Timeline({ events }: Props) {
  const [view, setView] = useState<TimelineView>(() => readStoredView());

  useEffect(() => {
    writeStoredView(view);
  }, [view]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      <header
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'flex-end',
          gap: 4,
        }}
      >
        <ToggleButton
          active={view === 'by-entity'}
          onClick={() => setView('by-entity')}
        >
          By entity
        </ToggleButton>
        <ToggleButton
          active={view === 'chronological'}
          onClick={() => setView('chronological')}
        >
          Chronological
        </ToggleButton>
      </header>
      {view === 'by-entity' ? (
        <ByEntityView events={events} />
      ) : (
        <ChronologicalView events={events} />
      )}
    </div>
  );
}

function ByEntityView({ events }: Props) {
  const sections = groupEventsForTimeline(events);
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {sections.map((section) => (
        <EntitySection
          key={`${section.entity.kind}:${section.entity.id}`}
          section={section}
        />
      ))}
    </div>
  );
}

function ChronologicalView({ events }: Props) {
  const rows = foldAdjacentSameKey(events);
  return (
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
      {rows.map((row) => (
        <li key={row.anchor.idempotency_key}>
          <CollapsedGroupRow row={row} />
        </li>
      ))}
    </ul>
  );
}

interface ToggleButtonProps {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function ToggleButton({ active, onClick, children }: ToggleButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      style={{
        background: active ? 'var(--surface-2)' : 'transparent',
        color: active ? 'var(--fg)' : 'var(--fg-muted)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--r-sm)',
        padding: '4px 10px',
        fontSize: 11,
        fontWeight: 600,
        cursor: 'pointer',
        fontFamily: 'inherit',
        letterSpacing: '0.02em',
      }}
    >
      {children}
    </button>
  );
}
