import { describe, it, expect } from 'vitest';
import { groupEventsForTimeline, foldAdjacentSameKey } from './grouping';
import type { EventEnvelope } from 'api-client-ts';

function mkEv(
  id: string,
  groupKey: string,
  ts: string,
  entity?: { kind: string; id: string; display_name: string }
): EventEnvelope {
  // We cast through `unknown` because the generated `event` type is
  // `Record<string, never>` (openapi-typescript's mapping of an
  // unspecified Object). The test only needs `event.timestamp` to
  // drive the section-sort, so we don't model every GameEvent variant.
  return {
    idempotency_key: id,
    raw_line: '',
    source: 'live',
    source_offset: 0,
    event: { type: 'attachment_received', timestamp: ts } as unknown as Record<
      string,
      never
    >,
    metadata: {
      group_key: groupKey,
      source: 'observed',
      confidence: 1.0,
      primary_entity:
        entity != null
          ? {
              kind: entity.kind as 'item',
              id: entity.id,
              display_name: entity.display_name,
            }
          : { kind: 'item', id: 'i1', display_name: 'helmet' },
      field_provenance: {},
      inference_inputs: [],
      rule_id: null,
    },
  };
}

function mkEvNoMetadata(id: string): EventEnvelope {
  return {
    idempotency_key: id,
    raw_line: '',
    source: 'live',
    source_offset: 0,
    event: null as unknown as Record<string, never>,
    metadata: null,
  };
}

describe('foldAdjacentSameKey', () => {
  it('collapses three adjacent same-group_key events into one row with count=3', () => {
    const rows = foldAdjacentSameKey([
      mkEv('1', 'a', 't1'),
      mkEv('2', 'a', 't2'),
      mkEv('3', 'a', 't3'),
      mkEv('4', 'b', 't4'),
    ]);
    expect(rows).toHaveLength(2);
    expect(rows[0].count).toBe(3);
    expect(rows[0].members).toHaveLength(3);
    expect(rows[1].count).toBe(1);
  });

  it('does not collapse same-key events when a different key is between them', () => {
    const rows = foldAdjacentSameKey([
      mkEv('1', 'a', 't1'),
      mkEv('2', 'b', 't2'),
      mkEv('3', 'a', 't3'),
    ]);
    expect(rows).toHaveLength(3);
  });

  it('handles missing metadata gracefully', () => {
    expect(foldAdjacentSameKey([mkEvNoMetadata('x')])).toHaveLength(1);
  });
});

describe('groupEventsForTimeline', () => {
  it('groups events by entity (kind:id) and sorts sections by last activity desc', () => {
    const events: EventEnvelope[] = [
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
      mkEv('3', 'c', '2026-05-17T14:02:00Z', {
        kind: 'vehicle',
        id: 'v1',
        display_name: 'Cutlass',
      }),
    ];
    const sections = groupEventsForTimeline(events);
    expect(sections[0].entity.id).toBe('Jim');
    expect(sections[1].entity.id).toBe('v1');
    expect(sections[1].events).toHaveLength(2);
    expect(sections[1].rows.length).toBeGreaterThan(0);
  });

  it('omits events without metadata.primary_entity', () => {
    expect(groupEventsForTimeline([mkEvNoMetadata('x')])).toEqual([]);
  });
});
