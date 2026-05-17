import { describe, it, expect, beforeEach, beforeAll } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Timeline, TIMELINE_VIEW_STORAGE_KEY } from './Timeline';
import type { EventEnvelope } from 'api-client-ts';

// JSDOM as bundled with this vitest install exposes `window` but not
// `window.localStorage`. Install a minimal in-memory shim so the tests
// can drive the same persistence path the production code uses.
beforeAll(() => {
  if (typeof window.localStorage === 'undefined') {
    const store = new Map<string, string>();
    const storage: Storage = {
      get length() {
        return store.size;
      },
      clear: () => store.clear(),
      getItem: (k) => (store.has(k) ? store.get(k)! : null),
      key: (i) => Array.from(store.keys())[i] ?? null,
      removeItem: (k) => {
        store.delete(k);
      },
      setItem: (k, v) => {
        store.set(k, String(v));
      },
    };
    Object.defineProperty(window, 'localStorage', {
      value: storage,
      configurable: true,
    });
  }
});

function mkEv(
  id: string,
  groupKey: string,
  ts: string,
  entity: { kind: 'vehicle' | 'player'; id: string; display_name: string }
): EventEnvelope {
  return {
    idempotency_key: id,
    raw_line: '',
    source: 'live',
    source_offset: 0,
    event: {
      type: 'attachment_received',
      timestamp: ts,
    } as unknown as Record<string, never>,
    metadata: {
      group_key: groupKey,
      source: 'observed',
      confidence: 1.0,
      primary_entity: entity,
      field_provenance: {},
      inference_inputs: [],
      rule_id: null,
    },
  };
}

const sampleEvents: EventEnvelope[] = [
  mkEv('1', 'a', '2026-05-17T14:00:00Z', {
    kind: 'vehicle',
    id: 'v1',
    display_name: 'Cutlass',
  }),
  mkEv('2', 'b', '2026-05-17T14:05:00Z', {
    kind: 'player',
    id: 'Jim',
    display_name: 'Jim',
  }),
];

describe('Timeline', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('groups by entity by default', () => {
    render(<Timeline events={sampleEvents} />);
    // The by-entity view renders one EntitySection per entity, each
    // with the entity's display_name in the header.
    expect(screen.getByText('Cutlass')).toBeInTheDocument();
    expect(screen.getByText('Jim')).toBeInTheDocument();
  });

  it('switches to chronological view when the toggle is clicked', async () => {
    const user = userEvent.setup();
    render(<Timeline events={sampleEvents} />);
    await user.click(screen.getByRole('button', { name: /chronological/i }));
    // The by-entity section titles disappear in chronological mode —
    // CollapsedGroupRow renders the event type, not the entity name.
    expect(screen.queryByText('Cutlass')).toBeNull();
    expect(screen.queryByText('Jim')).toBeNull();
    // Both events render as their own rows (no folding — distinct keys).
    expect(screen.getAllByText('attachment_received')).toHaveLength(2);
  });

  it('persists the view choice in localStorage', async () => {
    const user = userEvent.setup();
    const { unmount } = render(<Timeline events={sampleEvents} />);
    await user.click(screen.getByRole('button', { name: /chronological/i }));
    expect(window.localStorage.getItem(TIMELINE_VIEW_STORAGE_KEY)).toBe(
      'chronological'
    );
    unmount();
    render(<Timeline events={sampleEvents} />);
    // After remount, chronological is preserved: entity titles stay
    // absent, chronological rows are visible.
    expect(screen.queryByText('Cutlass')).toBeNull();
    expect(screen.getAllByText('attachment_received')).toHaveLength(2);
  });

  it('defaults to by-entity when localStorage holds an unknown value', () => {
    window.localStorage.setItem(TIMELINE_VIEW_STORAGE_KEY, 'bogus');
    render(<Timeline events={sampleEvents} />);
    expect(screen.getByText('Cutlass')).toBeInTheDocument();
    expect(screen.getByText('Jim')).toBeInTheDocument();
  });
});
