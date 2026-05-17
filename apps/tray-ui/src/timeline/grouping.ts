/**
 * Timeline grouping + folding helpers.
 *
 * The tray timeline supports two views:
 *   - By-entity (default): events bucketed by `metadata.primary_entity`,
 *     each section sorted by most-recent activity. See
 *     `groupEventsForTimeline`.
 *   - Chronological: a flat list with adjacent same-key rows folded
 *     into a single count badge. See `foldAdjacentSameKey`.
 *
 * Both helpers are pure — they take the wire `EventEnvelope[]` from the
 * generated client and return view-ready shapes. UI components stay
 * presentational; testability stays high.
 */

import type { EventEnvelope, EntityRef } from 'api-client-ts';

export interface TimelineRow {
  /** Group key the row was folded on. Falls back to the envelope's
   *  `idempotency_key` prefixed with `__` when the envelope has no
   *  metadata, so rows still stay unique on screen. */
  key: string;
  /** Number of envelopes folded into this row. `1` means an
   *  unfolded row; >1 means the count badge should render. */
  count: number;
  /** All envelopes folded into the row, in input order. */
  members: EventEnvelope[];
  /** First envelope in the run — the one the row's summary is drawn
   *  from. The rest are revealed on drill-in. */
  anchor: EventEnvelope;
}

export interface EntitySection {
  /** Primary entity all events in this section are about. */
  entity: EntityRef;
  /** Timestamp of the most recent event in the section. Used to sort
   *  sections — newest first. Empty string when no event in the
   *  section carries an `event.timestamp`. */
  lastActivity: string;
  /** Adjacent-folded rows derived from `events`. */
  rows: TimelineRow[];
  /** Raw envelopes assigned to this section, in input order. */
  events: EventEnvelope[];
}

/**
 * Collapse runs of envelopes with the same `metadata.group_key` into
 * a single row carrying a `count` and the full member list. Only
 * adjacent envelopes fold — an interruption by a different key
 * breaks the run, mirroring how `Game.log` is read top-to-bottom.
 *
 * Envelopes without metadata get a unique key derived from their
 * idempotency key, which guarantees they never fold with neighbours.
 */
export function foldAdjacentSameKey(events: EventEnvelope[]): TimelineRow[] {
  const rows: TimelineRow[] = [];
  for (const ev of events) {
    const key = ev.metadata?.group_key ?? `__${ev.idempotency_key}`;
    const last = rows.length > 0 ? rows[rows.length - 1] : null;
    if (last && last.key === key) {
      rows[rows.length - 1] = {
        ...last,
        count: last.count + 1,
        members: [...last.members, ev],
      };
    } else {
      rows.push({ key, count: 1, members: [ev], anchor: ev });
    }
  }
  return rows;
}

function entityKey(entity: EntityRef): string {
  return `${entity.kind}:${entity.id}`;
}

/**
 * Group envelopes by their primary entity (`kind:id`). Within each
 * section the events keep input order; sections themselves are
 * sorted by `lastActivity` (the newest `event.timestamp` in the
 * bucket), newest first.
 *
 * Envelopes without `metadata.primary_entity` are silently dropped:
 * the entity-first view has nothing to anchor them on. The
 * chronological view (see `foldAdjacentSameKey`) is the fallback for
 * those.
 */
export function groupEventsForTimeline(
  events: EventEnvelope[]
): EntitySection[] {
  const byEntity = new Map<string, EntitySection>();
  for (const ev of events) {
    const entity = ev.metadata?.primary_entity;
    if (entity == null) continue;
    const id = entityKey(entity);
    // `event` is `Record<string, never>` in the generated schema
    // (openapi-typescript's mapping of an unspecified Object). The
    // actual wire shape carries `event.timestamp`, which is what we
    // sort by. Cast through `unknown` to read it without inventing
    // an alternative GameEvent typing in tray-ui.
    const ts =
      ((ev.event as unknown as { timestamp?: string } | null)?.timestamp ?? '');
    const existing = byEntity.get(id);
    if (existing == null) {
      byEntity.set(id, {
        entity,
        lastActivity: ts,
        rows: [],
        events: [ev],
      });
    } else {
      const lastActivity =
        ts > existing.lastActivity ? ts : existing.lastActivity;
      byEntity.set(id, {
        ...existing,
        lastActivity,
        events: [...existing.events, ev],
      });
    }
  }

  const sections = Array.from(byEntity.values()).map((s) => ({
    ...s,
    rows: foldAdjacentSameKey(s.events),
  }));
  sections.sort((a, b) => b.lastActivity.localeCompare(a.lastActivity));
  return sections;
}
