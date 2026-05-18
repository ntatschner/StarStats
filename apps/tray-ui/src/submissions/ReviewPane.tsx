/**
 * Side panel listing unknown-shape candidates from the local SQLite
 * cache. Each row exposes the shape header (hash, occurrence count,
 * interest score, optional shell tag), the most-recent raw example,
 * PII toggles per detected token, free-text "suggested event name"
 * + "notes", and Submit / Dismiss buttons.
 *
 * Submit applies the chosen redactions to the raw example before
 * handing the payload to the parent's `onSubmit`; Dismiss bubbles the
 * shape hash up so the parent can mark it dismissed in storage.
 *
 * Rows sort by `interest_score * occurrence_count` desc — most
 * actionable shapes float to the top.
 */

import { useState } from 'react';
import { PiiToggle, type PiiToken } from './PiiToggle';

export interface UnknownShape {
  shape_hash: string;
  raw_example: string;
  interest_score: number;
  occurrence_count: number;
  shell_tag?: string | null;
  detected_pii: PiiToken[];
}

export interface SubmitPayload {
  shape_hash: string;
  /** raw example with the user's chosen redactions applied. */
  raw_example: string;
  suggested_event_name?: string;
  notes?: string;
  /** keyed by `${kind}@${start}` — preserves the per-token decision
   *  so the caller can include it in the submission record. */
  redactions: Record<string, boolean>;
}

interface Props {
  shapes: UnknownShape[];
  onSubmit: (payload: SubmitPayload) => void;
  onDismiss: (shapeHash: string) => void;
}

export function ReviewPane({ shapes, onSubmit, onDismiss }: Props) {
  const sorted = [...shapes].sort(
    (a, b) =>
      b.interest_score * b.occurrence_count - a.interest_score * a.occurrence_count
  );
  if (sorted.length === 0) {
    return <div className="review-pane-empty">No unknown lines to review.</div>;
  }
  return (
    <div className="review-pane">
      {sorted.map((s) => (
        <ShapeRow
          key={s.shape_hash}
          shape={s}
          onSubmit={onSubmit}
          onDismiss={onDismiss}
        />
      ))}
    </div>
  );
}

function ShapeRow({
  shape,
  onSubmit,
  onDismiss,
}: {
  shape: UnknownShape;
  onSubmit: (p: SubmitPayload) => void;
  onDismiss: (s: string) => void;
}) {
  const [redactions, setRedactions] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(
      shape.detected_pii.map((t) => [tokenKey(t), t.default_redact])
    )
  );
  const [suggestedName, setSuggestedName] = useState('');
  const [notes, setNotes] = useState('');

  const submit = () => {
    onSubmit({
      shape_hash: shape.shape_hash,
      raw_example: applyRedactions(shape.raw_example, shape.detected_pii, redactions),
      suggested_event_name: suggestedName || undefined,
      notes: notes || undefined,
      redactions,
    });
  };

  return (
    <div className="shape-row" data-testid="shape-row">
      <header>
        <code>{shape.shape_hash}</code>
        <span className="badge">×{shape.occurrence_count}</span>
        <span className="badge interest">{shape.interest_score}</span>
        {shape.shell_tag && (
          <span className="badge tag">&lt;{shape.shell_tag}&gt;</span>
        )}
      </header>
      <pre className="raw">{shape.raw_example}</pre>
      {shape.detected_pii.length > 0 && (
        <div className="pii-toggles">
          {shape.detected_pii.map((t) => (
            <PiiToggle
              key={tokenKey(t)}
              token={t}
              onChange={(redact) =>
                setRedactions((r) => ({ ...r, [tokenKey(t)]: redact }))
              }
            />
          ))}
        </div>
      )}
      <input
        type="text"
        placeholder="Suggested event name (optional)"
        value={suggestedName}
        onChange={(e) => setSuggestedName(e.target.value)}
      />
      <textarea
        placeholder="Notes for the rule author (optional)"
        value={notes}
        onChange={(e) => setNotes(e.target.value)}
      />
      <div className="actions">
        <button type="button" onClick={submit}>
          Submit
        </button>
        <button type="button" onClick={() => onDismiss(shape.shape_hash)}>
          Dismiss
        </button>
      </div>
    </div>
  );
}

function tokenKey(t: PiiToken): string {
  return `${t.kind}@${t.start}`;
}

/**
 * Apply the user's redaction choices to the raw example. Tokens are
 * processed right-to-left so earlier offsets aren't shifted by a
 * replacement of different length further along the string.
 */
function applyRedactions(
  raw: string,
  tokens: PiiToken[],
  redactions: Record<string, boolean>
): string {
  const sorted = [...tokens].sort((a, b) => b.start - a.start);
  let result = raw;
  for (const t of sorted) {
    if (redactions[tokenKey(t)]) {
      result = result.slice(0, t.start) + t.suggested_redaction + result.slice(t.end);
    }
  }
  return result;
}
