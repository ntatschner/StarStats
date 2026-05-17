import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { EntitySection } from './EntitySection';
import type { EntitySection as EntitySectionData } from './grouping';
import type { EventEnvelope } from 'api-client-ts';

function mkEv(id: string, groupKey: string): EventEnvelope {
  return {
    idempotency_key: id,
    raw_line: '',
    source: 'live',
    source_offset: 0,
    event: { type: 'attachment_received', timestamp: '2026-05-17T14:00:00Z' } as unknown as Record<string, never>,
    metadata: {
      group_key: groupKey,
      source: 'observed',
      confidence: 1.0,
      primary_entity: { kind: 'vehicle', id: 'v1', display_name: 'Cutlass' },
      field_provenance: {},
      inference_inputs: [],
      rule_id: null,
    },
  };
}

function mkSection(eventCount: number): EntitySectionData {
  const events = Array.from({ length: eventCount }, (_, i) =>
    mkEv(`e${i}`, `gk${i}`)
  );
  return {
    entity: { kind: 'vehicle', id: 'v1', display_name: 'Cutlass' },
    lastActivity: '2026-05-17T14:00:00Z',
    events,
    rows: events.map((ev, i) => ({
      key: `gk${i}`,
      count: 1,
      members: [ev],
      anchor: ev,
    })),
  };
}

describe('EntitySection', () => {
  it('renders the entity display_name as the section title', () => {
    render(<EntitySection section={mkSection(3)} />);
    expect(screen.getByText('Cutlass')).toBeInTheDocument();
  });

  it('shows the event count in the header', () => {
    render(<EntitySection section={mkSection(4)} />);
    expect(screen.getByText(/4 events/i)).toBeInTheDocument();
  });

  it('uses singular "1 event" when only one event is present', () => {
    render(<EntitySection section={mkSection(1)} />);
    expect(screen.getByText(/1 event\b/i)).toBeInTheDocument();
  });

  it('hides the row list when collapsed and reveals it when expanded', async () => {
    const user = userEvent.setup();
    render(<EntitySection section={mkSection(2)} initialExpanded={false} />);
    expect(screen.queryByRole('list')).toBeNull();
    await user.click(screen.getByRole('button', { name: /cutlass/i }));
    expect(screen.getByRole('list')).toBeInTheDocument();
  });

  it('is expanded by default', () => {
    render(<EntitySection section={mkSection(2)} />);
    expect(screen.getByRole('list')).toBeInTheDocument();
  });
});
