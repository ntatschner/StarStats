"""SQLite schema and connection. Append-only event log plus snapshot tables."""
from __future__ import annotations

import sqlite3
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

SCHEMA = """
CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    type        TEXT    NOT NULL,
    timestamp   TEXT    NOT NULL,
    raw         TEXT    NOT NULL,
    payload     TEXT    NOT NULL,
    inserted_at TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_events_type_ts ON events(type, timestamp);

CREATE TABLE IF NOT EXISTS hangar_snapshot (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    captured_at TEXT    NOT NULL DEFAULT (datetime('now')),
    payload     TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS profile_snapshot (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    captured_at TEXT    NOT NULL DEFAULT (datetime('now')),
    payload     TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS tail_cursor (
    path        TEXT PRIMARY KEY,
    inode       TEXT,
    offset      INTEGER NOT NULL DEFAULT 0,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
"""


def init_db(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(path) as conn:
        conn.executescript(SCHEMA)


@contextmanager
def connect(path: Path) -> Iterator[sqlite3.Connection]:
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    try:
        yield conn
        conn.commit()
    finally:
        conn.close()
