"""Resumable Game.log tailer.

Tracks byte offset in `tail_cursor` so restarts pick up where we left
off. Re-opens the file on truncation (which happens at game launch).
"""
from __future__ import annotations

import sqlite3
import time
from collections.abc import Iterator
from pathlib import Path


def _read_cursor(conn: sqlite3.Connection, path: Path) -> int:
    row = conn.execute("SELECT offset FROM tail_cursor WHERE path = ?", (str(path),)).fetchone()
    return int(row[0]) if row else 0


def _write_cursor(conn: sqlite3.Connection, path: Path, offset: int) -> None:
    conn.execute(
        "INSERT INTO tail_cursor(path, offset) VALUES (?, ?) "
        "ON CONFLICT(path) DO UPDATE SET offset=excluded.offset, updated_at=datetime('now')",
        (str(path), offset),
    )
    conn.commit()


def tail(path: Path, conn: sqlite3.Connection, poll_seconds: float = 1.0) -> Iterator[str]:
    """Yield each new line appended to `path`, resuming from the saved cursor."""
    offset = _read_cursor(conn, path)
    while True:
        if not path.exists():
            time.sleep(poll_seconds)
            continue

        size = path.stat().st_size
        if size < offset:
            # Log was rotated/truncated (typical at game launch). Restart.
            offset = 0

        if size == offset:
            time.sleep(poll_seconds)
            continue

        with path.open("r", encoding="utf-8", errors="replace") as f:
            f.seek(offset)
            for line in f:
                if not line.endswith("\n"):
                    # Partial line — wait for more data.
                    break
                offset += len(line.encode("utf-8", errors="replace"))
                yield line
            _write_cursor(conn, path, offset)
