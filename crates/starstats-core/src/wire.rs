//! Wire-format types shared by client and server. Anything that
//! crosses the network lives here.
//!
//! Stability rule: once a field is on the wire, **never remove or
//! repurpose it**. Add new optional fields. Bump `schema_version` on
//! breaking changes (none planned for v1).

use crate::events::GameEvent;
use crate::metadata::EventMetadata;
use serde::{Deserialize, Serialize};

/// Single event with the metadata the server needs for dedupe and
/// trust scoring.
///
/// `Eq` is dropped because `GameEvent::AttachmentReceived` carries an
/// `f64` for elapsed seconds, which only implements `PartialEq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Stable event ID — derived by the client from `(line_offset, content)`
    /// so replays of the same line produce the same ID. UUIDv7 preferred.
    pub idempotency_key: String,

    /// Raw log line as it appeared in `Game.log`. Kept so the server
    /// can re-parse with newer rules without asking the client to
    /// re-upload.
    pub raw_line: String,

    /// Parsed event, if the client could parse it. May be `None` for
    /// lines the client recognised structurally but couldn't classify.
    pub event: Option<GameEvent>,

    /// Path of the source `Game.log` (relative to install root) — used
    /// to distinguish `LIVE/` from `PTU/` from `EPTU/` etc.
    pub source: LogSource,

    /// Byte offset within the source file. Lets the server reconstruct
    /// ordering even across out-of-order batch arrivals.
    pub source_offset: u64,

    /// Cross-cutting metadata stamped by the client (or by the server
    /// during the schema-v1 grace window). Optional on the wire so
    /// envelopes produced by pre-v2 clients still deserialise; the
    /// server back-fills a default observed metadata in that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<EventMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSource {
    Live,
    Ptu,
    Eptu,
    Hotfix,
    Tech,
    Other,
}

/// One client → server batch. Compressed (zstd) on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestBatch {
    /// Schema version — server rejects unknown versions with 400.
    pub schema_version: u16,

    /// Unique batch ID for tracing / dedupe.
    pub batch_id: String,

    /// Game build the events came from (from `<Init>` or `FileVersion`
    /// banner). Lets the server route to the correct parser revision
    /// when the spec drifts between patches.
    pub game_build: Option<String>,

    /// Player handle — claimed by the client. Server cross-checks
    /// against the bearer token's identity claims; mismatch → reject.
    pub claimed_handle: String,

    pub events: Vec<EventEnvelope>,
}

impl IngestBatch {
    /// Bumped to 2 when `EventEnvelope.metadata` was added. The server
    /// accepts both v1 (no metadata, synthesised server-side) and v2
    /// during the grace window described in the design spec.
    pub const CURRENT_SCHEMA_VERSION: u16 = 2;
}

/// One owned ship pulled from RSI's hangar / pledges page.
///
/// Fields are deliberately conservative: `name` is the only thing
/// guaranteed by RSI's HTML; manufacturer/kind/insurance are best-effort
/// and `None` when the upstream record is half-formed. The client
/// normalises whitespace and drops empty strings before serialising —
/// the server should never see `Some("")`.
///
/// `pledge_id` is RSI's internal record ID (the `data-pledge-id`
/// attribute on the pledge card). When present it lets dedupe key on a
/// stable identifier across snapshots; absence falls back to
/// `(name, manufacturer)` heuristic comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HangarShip {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pledge_id: Option<String>,
    /// "ship", "ground vehicle", "skin", "upgrade" etc. Free-form —
    /// we don't enumerate because RSI's classification drifts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Body of `POST /v1/me/hangar`. The tray client builds this after
/// scraping RSI; the server stamps `captured_at` server-side and
/// stores the snapshot keyed on the requesting user.
///
/// Empty `ships` is a valid (and important) signal: it can mean
/// "user has no hangar yet" OR "the parser found nothing on this
/// page" — distinguishing the two is the client's job (it shouldn't
/// POST a parser-failure as an empty hangar).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HangarPushRequest {
    /// Schema version — bumped on breaking changes. Currently `1`.
    pub schema_version: u16,
    pub ships: Vec<HangarShip>,
}

/// Per-user UI preferences. Stored as JSONB on `users.preferences`
/// and surfaced through `GET/PUT /v1/me/preferences`. Forward-extensible:
/// every field is optional + skip-on-None so adding new fields
/// (notifications, accent intensity, name plate, etc.) does not break
/// older clients that round-trip the value.
///
/// `theme` is intentionally `Option<String>` (not an enum) so unknown
/// values round-trip cleanly when the wire crate is older than the
/// server. The route layer enforces the allowlist at write time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{GameEvent, JoinPu, PlayerDeath};
    use crate::metadata::{stamp, EntityKind};

    #[test]
    fn round_trips_through_json() {
        let batch = IngestBatch {
            schema_version: 1,
            batch_id: "01934f5a-3b2a-7000-a000-000000000000".into(),
            game_build: Some("4.7.178.50402".into()),
            claimed_handle: "TheCodeSaiyan".into(),
            events: vec![EventEnvelope {
                idempotency_key: "evt-1".into(),
                raw_line: "<...>".into(),
                event: Some(GameEvent::JoinPu(JoinPu {
                    timestamp: "2026-05-02T21:14:23.189Z".into(),
                    address: "1.2.3.4".into(),
                    port: 64300,
                    shard: "pub_euw1b".into(),
                    location_id: "1".into(),
                })),
                source: LogSource::Live,
                source_offset: 0,
                metadata: None,
            }],
        };
        let s = serde_json::to_string(&batch).unwrap();
        let parsed: IngestBatch = serde_json::from_str(&s).unwrap();
        assert_eq!(batch, parsed);
    }

    #[test]
    fn envelope_with_metadata_round_trips() {
        let ev = GameEvent::PlayerDeath(PlayerDeath {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            body_class: "body_01_noMagicPocket".into(),
            body_id: "1".into(),
            zone: None,
        });
        let env = EventEnvelope {
            idempotency_key: "evt-1".into(),
            raw_line: "<...>".into(),
            event: Some(ev.clone()),
            source: LogSource::Live,
            source_offset: 0,
            metadata: Some(stamp(&ev, Some("alice"))),
        };
        let s = serde_json::to_string(&env).unwrap();
        let parsed: EventEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(env, parsed);
        let metadata = parsed.metadata.expect("metadata must survive round-trip");
        assert_eq!(metadata.primary_entity.kind, EntityKind::Player);
    }

    #[test]
    fn envelope_without_metadata_still_deserialises() {
        // Wire form produced by a pre-v2 client: no `metadata` key.
        let legacy = r#"{
            "idempotency_key": "evt-1",
            "raw_line": "<...>",
            "event": null,
            "source": "live",
            "source_offset": 0
        }"#;
        let parsed: EventEnvelope = serde_json::from_str(legacy).unwrap();
        assert!(parsed.metadata.is_none());
    }

    #[test]
    fn schema_version_bumped_to_two() {
        assert_eq!(IngestBatch::CURRENT_SCHEMA_VERSION, 2);
    }

    #[test]
    fn hangar_push_request_round_trips_through_json() {
        let req = HangarPushRequest {
            schema_version: 1,
            ships: vec![
                HangarShip {
                    name: "Aegis Avenger Titan".into(),
                    manufacturer: Some("Aegis Dynamics".into()),
                    pledge_id: Some("12345678".into()),
                    kind: Some("ship".into()),
                },
                HangarShip {
                    name: "Greycat PTV".into(),
                    manufacturer: None,
                    pledge_id: None,
                    kind: None,
                },
            ],
        };
        let s = serde_json::to_string(&req).unwrap();
        let parsed: HangarPushRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, parsed);

        // Optional fields with `None` should be omitted from the wire
        // form (skip_serializing_if), keeping the payload lean and
        // distinguishing absent from `null`. Each optional key appears
        // exactly once across the two ships (only the first carries it).
        assert_eq!(s.matches("\"manufacturer\"").count(), 1);
        assert_eq!(s.matches("\"pledge_id\"").count(), 1);
        assert_eq!(s.matches("\"kind\"").count(), 1);
    }
}
