/**
 * Tray-UI host for the unknown-line review queue. Owns the data
 * lifecycle: polls `list_unknown_lines` on mount + on demand, adapts
 * the SQLite row shape into the `UnknownShape` the ReviewPane
 * consumes, dispatches Submit / Dismiss back through the Tauri
 * bridge, and refreshes once a write lands.
 *
 * Kept in a sibling file (not inline in App.tsx) so the
 * data-fetching logic is testable with a mocked `api.*` surface
 * without spinning the whole app shell.
 */

import { useCallback, useEffect, useState } from 'react';
import { api, type UnknownLine } from '../api';
import { ReviewPane, type SubmitPayload, type UnknownShape } from './ReviewPane';

interface Props {
  /** Bumped by the parent when it wants the pane to refetch (e.g.
   *  after the badge polls and detects a new count). Optional —
   *  internal state changes already trigger a refetch via the
   *  Submit/Dismiss callbacks. */
  refreshKey?: number;
  /** Notifies the parent when the local cache count changes, so the
   *  badge stays in sync without an extra round-trip. */
  onCountChange?: (count: number) => void;
}

export function SubmissionsPane({ refreshKey, onCountChange }: Props) {
  const [shapes, setShapes] = useState<UnknownShape[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const rows = await api.listUnknownLines();
      const adapted = rows.map(rowToShape);
      setShapes(adapted);
      setError(null);
      onCountChange?.(adapted.length);
    } catch (e) {
      setError(String(e));
    }
  }, [onCountChange]);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshKey]);

  const onSubmit = useCallback(
    async (payload: SubmitPayload) => {
      // Find the source row so we can carry through the fields the
      // server expects but the review UI doesn't surface (channel,
      // partial_structured, context, etc.). The user only edited
      // `raw_example`, `suggested_event_name`, and `notes`.
      const row = shapes.find((s) => s.shape_hash === payload.shape_hash);
      if (!row) return;
      try {
        await api.submitUnknownLines([
          {
            shape_hash: payload.shape_hash,
            raw_examples: [payload.raw_example],
            shell_tag: row.shell_tag ?? undefined,
            suggested_event_name: payload.suggested_event_name,
            notes: payload.notes,
            channel: 'channel_live',
            occurrence_count: row.occurrence_count,
            // `client_anon_id` is owned by the server submission
            // surface in a later phase — empty string here lets the
            // wire shape round-trip; the bearer token already
            // identifies the device for dedup purposes.
            client_anon_id: '',
          },
        ]);
        await refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [shapes, refresh]
  );

  const onDismiss = useCallback(
    async (shapeHash: string) => {
      try {
        await api.dismissUnknownLine(shapeHash);
        await refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [refresh]
  );

  return (
    <div className="submissions-pane">
      {error && <div className="error">Error: {error}</div>}
      <ReviewPane shapes={shapes} onSubmit={onSubmit} onDismiss={onDismiss} />
    </div>
  );
}

/**
 * Adapt one SQLite `UnknownLine` row into the `UnknownShape` the
 * review pane renders. We pick the most-recent raw_line off the row
 * (storage caches up to RAW_EXAMPLES_CAP) — that's `raw_line`
 * itself, which is the freshest capture per the storage upsert
 * semantics.
 */
function rowToShape(row: UnknownLine): UnknownShape {
  return {
    shape_hash: row.shape_hash,
    raw_example: row.raw_line,
    interest_score: row.interest_score,
    occurrence_count: row.occurrence_count,
    shell_tag: row.shell_tag,
    detected_pii: row.detected_pii,
  };
}
