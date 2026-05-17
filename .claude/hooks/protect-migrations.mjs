#!/usr/bin/env node
/**
 * PreToolUse hook — blocks edits to any existing migration file in
 * `crates/starstats-server/migrations/`. Migrations are append-only
 * by design (see CLAUDE.md "Architecture Invariants"): once a file
 * is committed, modifying it would break replay on existing
 * databases.
 *
 * Adding a NEW migration file (e.g. `0028_*.sql`) is fine — the hook
 * only fires on Edit (existing files), not Write (new files), per
 * the settings.json matcher.
 *
 * Hook contract (Claude Code):
 *   - stdin = JSON `{tool_input: {file_path}, ...}`
 *   - exit 0 = allow; exit 2 + stderr = block (Claude sees the message)
 */
import { readFileSync } from 'node:fs';
import { posix, sep } from 'node:path';

function readStdin() {
  try {
    return readFileSync(0, 'utf8');
  } catch {
    return '';
  }
}

const raw = readStdin();
if (!raw.trim()) process.exit(0);

let payload;
try {
  payload = JSON.parse(raw);
} catch {
  process.exit(0);
}

const filePath = payload?.tool_input?.file_path;
if (!filePath) process.exit(0);

// Normalise Windows backslashes so the pattern check works the same
// on every platform.
const normalised = filePath.split(sep).join(posix.sep);
const isMigration = /\/crates\/starstats-server\/migrations\/\d{4}_[^/]+\.sql$/.test(
  normalised,
);
if (!isMigration) process.exit(0);

process.stderr.write(
  [
    'BLOCKED: migrations in crates/starstats-server/migrations/ are append-only.',
    '',
    'Modifying an existing 00NN_*.sql breaks replay on databases that',
    'already applied it. Add a NEW migration file (next number) that',
    'expresses the change as a forward-only diff:',
    '',
    '  crates/starstats-server/migrations/00NN_<descriptive_name>.sql',
    '',
    'Conventions (CLAUDE.md): IF NOT EXISTS, NULL defaults, no DROPs,',
    'no NOT NULL on populated columns without DEFAULT. Backward-compat',
    'parsing in Rust uses #[serde(default)].',
  ].join('\n') + '\n',
);
process.exit(2);
