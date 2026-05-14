-- 0023_share_metadata.sql — Per-user share metadata (expiry + notes).
--
-- SpiceDB stores the share relationship itself (one
-- `stats_record:<owner>#share_with_user@user:<recipient>` row per
-- grant). It has no concept of time, so we keep all temporal /
-- annotation data here instead. The metadata is purely *advisory*
-- when read; enforcement is the read-time check in the friend_*
-- handlers, which 404 + lazily delete the SpiceDB row when
-- `expires_at < now()`.
--
-- The table is keyed on (lower(owner_handle), lower(recipient_handle))
-- so case mismatches collapse to the same row — RSI handles are
-- case-preserved on display but case-insensitive for lookup, and
-- SpiceDB ids inherit that.
--
-- No FK to `users`: a metadata row can outlive a deleted account
-- (the SpiceDB cleanup happens separately), and we don't want a
-- user-deletion path to take this table down with it.

CREATE TABLE IF NOT EXISTS share_metadata (
    owner_handle     TEXT        NOT NULL,
    recipient_handle TEXT        NOT NULL,
    -- NULL = share never expires (matches the previous wave's
    -- behaviour, which had no expiry concept at all).
    expires_at       TIMESTAMPTZ NULL,
    -- Free-text human note. Capped on write (validation in the
    -- handler) to keep the column bounded.
    note             TEXT        NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (lower(owner_handle), lower(recipient_handle))
);

-- Indexed lookup for expiry sweeps (a future background task can
-- batch-revoke expired rows; for now read-time enforcement covers
-- correctness, this index is forward-looking).
CREATE INDEX IF NOT EXISTS share_metadata_expires_at_idx
    ON share_metadata (expires_at)
    WHERE expires_at IS NOT NULL;

-- Reverse lookup for the inbound side (list_shared_with_me + the
-- friend read endpoints both filter by recipient).
CREATE INDEX IF NOT EXISTS share_metadata_recipient_lower_idx
    ON share_metadata (lower(recipient_handle));
