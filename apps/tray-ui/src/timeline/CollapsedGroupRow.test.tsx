import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CollapsedGroupRow } from './CollapsedGroupRow';
import type { TimelineRow } from './grouping';
import type { EventEnvelope, EventSource } from 'api-client-ts';

type FieldProvenanceLike =
  | { type: 'observed' }
  | { type: 'inferred_from'; source_event_ids: string[]; rule_id: string };

function mkEv(
  id: string,
  source: EventSource = 'observed',
  confidence = 1.0,
  fieldProvenance: Record<string, FieldProvenanceLike> = {}
): EventEnvelope {
  return {
    idempotency_key: id,
    raw_line: '',
    source: 'live',
    source_offset: 0,
    event: {
      type: 'attachment_received',
      timestamp: '2026-05-17T14:00:00Z',
    } as unknown as Record<string, never>,
    metadata: {
      group_key: 'gk',
      source,
      confidence,
      primary_entity: { kind: 'item', id: 'i1', display_name: 'helmet' },
      field_provenance: fieldProvenance,
      inference_inputs: [],
      rule_id: source === 'inferred' ? 'fuel_out_to_spawn' : null,
    },
  };
}

function mkRow(members: EventEnvelope[]): TimelineRow {
  return {
    key: 'gk',
    count: members.length,
    members,
    anchor: members[0],
  };
}

describe('CollapsedGroupRow', () => {
  it('renders a count badge when count > 1', () => {
    render(<CollapsedGroupRow row={mkRow([mkEv('a'), mkEv('b'), mkEv('c')])} />);
    expect(screen.getByText(/×\s*3/)).toBeInTheDocument();
  });

  it('does not render a count badge when count is 1', () => {
    render(<CollapsedGroupRow row={mkRow([mkEv('a')])} />);
    expect(screen.queryByText(/×/)).toBeNull();
  });

  it('renders an InferredBadge when the anchor is inferred', () => {
    render(
      <CollapsedGroupRow row={mkRow([mkEv('a', 'inferred', 0.6)])} />
    );
    expect(screen.getByText(/inferred/i)).toBeInTheDocument();
    expect(screen.getByText('60%')).toBeInTheDocument();
  });

  it('omits the InferredBadge for observed rows', () => {
    render(<CollapsedGroupRow row={mkRow([mkEv('a', 'observed')])} />);
    expect(screen.queryByText(/inferred/i)).toBeNull();
  });

  it('renders a field-level pill for each inferred_from field', () => {
    const ev = mkEv('a', 'inferred', 0.85, {
      zone: {
        type: 'inferred_from',
        source_event_ids: ['envC'],
        rule_id: 'zone_from_recent',
      },
      cause: {
        type: 'inferred_from',
        source_event_ids: ['envD'],
        rule_id: 'cause_from_context',
      },
    });
    render(<CollapsedGroupRow row={mkRow([ev])} />);
    expect(screen.getByText(/zone inferred/i)).toBeInTheDocument();
    expect(screen.getByText(/cause inferred/i)).toBeInTheDocument();
  });

  it('omits field-level pills for observed fields', () => {
    const ev = mkEv('a', 'observed', 1.0, {
      zone: { type: 'observed' },
    });
    render(<CollapsedGroupRow row={mkRow([ev])} />);
    expect(screen.queryByText(/zone inferred/i)).toBeNull();
  });

  it('reveals member events on drill-in', async () => {
    const user = userEvent.setup();
    const row = mkRow([mkEv('a'), mkEv('b'), mkEv('c')]);
    render(<CollapsedGroupRow row={row} />);
    // Before drill-in, the members listbox is not in the DOM.
    expect(screen.queryByTestId('group-row-members')).toBeNull();
    await user.click(screen.getByRole('button', { name: /×\s*3/ }));
    const members = screen.getByTestId('group-row-members');
    expect(members).toBeInTheDocument();
    // Three member rows, one per envelope.
    expect(members.querySelectorAll('li')).toHaveLength(3);
  });
});
