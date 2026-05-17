#!/usr/bin/env node
/**
 * PostToolUse hook — runs `cargo fmt` on a single touched `.rs` file
 * after Edit/Write/MultiEdit. Mirrors the CI gate (`cargo fmt --check`)
 * so the next Edit doesn't get blocked by a drift warning, and CI
 * doesn't fail on a one-line whitespace nit.
 *
 * Hook contract (Claude Code):
 *   - stdin = JSON `{tool_input: {file_path}, ...}`
 *   - exit 0 = pass (no-op or success); exit 2 + stderr = block
 *
 * Exits 0 silently for non-Rust paths, missing files, or fmt errors —
 * fmt failures shouldn't block the edit; CI will catch them.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';

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
if (!filePath || !filePath.endsWith('.rs')) process.exit(0);
if (!existsSync(filePath)) process.exit(0);

try {
  execFileSync('cargo', ['fmt', '--', filePath], {
    stdio: 'ignore',
    windowsHide: true,
  });
} catch {
  // Swallow — fmt errors (e.g. syntax-broken file mid-edit) shouldn't
  // block the workflow.
}
process.exit(0);
