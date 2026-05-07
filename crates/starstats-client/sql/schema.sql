-- Local SQLite schema for the StarStats tray client.
-- Append-only events + tail offset cursor.

CREATE TABLE IF NOT EXISTS events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    idempotency_key TEXT    NOT NULL UNIQUE,
    type            TEXT    NOT NULL,
    timestamp       TEXT    NOT NULL,
    raw             TEXT    NOT NULL,
    payload         TEXT    NOT NULL,
    log_source      TEXT    NOT NULL DEFAULT 'live',
    source_offset   INTEGER NOT NULL DEFAULT 0,
    inserted_at     TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_events_type_ts ON events(type, timestamp);
CREATE INDEX IF NOT EXISTS idx_events_inserted ON events(inserted_at);

CREATE TABLE IF NOT EXISTS tail_cursor (
    path        TEXT PRIMARY KEY,
    offset      INTEGER NOT NULL DEFAULT 0,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Cursor for "what's been shipped to the API server" — used by the
-- sync worker (Phase 1b). Distinct from tail_cursor (which tracks
-- "what's been read from disk").
CREATE TABLE IF NOT EXISTS sync_cursor (
    last_event_id INTEGER NOT NULL,
    updated_at    TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Lines that the structural parser recognised (gave us a stable
-- timestamp + event_name) but for which the classifier had no rule.
-- Dedupe by (log_source, event_name); we keep the most recent body
-- as a sample, plus first_seen / last_seen for forensic context, and
-- bump occurrences so the user can see which unknowns are common.
--
-- This table is the input for two later features:
--   1. UI surface — "you have N unknown event types, here they are"
--   2. Crowd-sourced rules — `sample_body` is what a user-submitted
--      regex would actually be tested against.
--
-- No-shell lines (banners, blanks, continuation lines) are NOT
-- recorded here — they're not actionable as parser rules.
CREATE TABLE IF NOT EXISTS unknown_event_samples (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    log_source   TEXT    NOT NULL,
    event_name   TEXT    NOT NULL,
    occurrences  INTEGER NOT NULL DEFAULT 1,
    first_seen   TEXT    NOT NULL DEFAULT (datetime('now')),
    last_seen    TEXT    NOT NULL DEFAULT (datetime('now')),
    sample_line  TEXT    NOT NULL,
    sample_body  TEXT    NOT NULL,
    UNIQUE(log_source, event_name)
);
CREATE INDEX IF NOT EXISTS idx_unknown_samples_occurrences
    ON unknown_event_samples (occurrences DESC, last_seen DESC);

-- Event names we deliberately ignore. Peer concept to "rules" — when
-- the rules engine ships, this is how users say "this is engine-
-- internal noise, never show it in the unknowns list, never propose a
-- rule for it."
--
-- `source` is informational: 'builtin' for the seeded defaults the
-- app ships, 'user' for entries added via the tray UI, 'community'
-- (future) for entries pulled from the central rules service.
--
-- The unique key is (event_name) — we don't currently scope by
-- log_source because engine internals fire identically on LIVE/PTU/
-- EPTU. If that ever stops being true, add log_source to the PK.
CREATE TABLE IF NOT EXISTS event_noise_list (
    event_name  TEXT    NOT NULL PRIMARY KEY,
    source      TEXT    NOT NULL DEFAULT 'user',
    added_at    TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Cached parser-definition manifest fetched from the server's
-- `GET /v1/parser-definitions`. Only one row at a time — `id = 1` is
-- a sentinel so an UPSERT replaces the cache instead of accumulating
-- stale rows. `payload_json` holds the full Manifest as a JSON
-- blob; the client deserialises + compiles on startup or after a
-- successful fetch.
CREATE TABLE IF NOT EXISTS parser_def_manifest (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    version       INTEGER NOT NULL,
    fetched_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    payload_json  TEXT    NOT NULL
);
