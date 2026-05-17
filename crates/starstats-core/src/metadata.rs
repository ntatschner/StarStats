//! Cross-cutting event metadata stamped on every `EventEnvelope`.
//!
//! The wire format carries the parsed `GameEvent` plus an
//! `EventMetadata` envelope that names the *primary entity* the event
//! is about, where the event came from (observed in the log,
//! inferred, or synthesized), and how confident we are in it. The
//! metadata composes higher-level features — timeline grouping,
//! supersession of inferred rows by later observed rows, per-field
//! provenance trails — without reshaping the strongly-typed
//! `GameEvent` enum.
//!
//! Design rule: metadata is purely additive. Adding a new
//! `EntityKind` or `EventSource` variant must not break older clients
//! that round-trip the envelope verbatim. Unknown variants are
//! rejected at deserialise time (serde default), which is what we
//! want for a closed vocabulary.

use crate::events::GameEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Categorical kind of the primary entity an event is about.
///
/// Closed vocabulary — every `GameEvent` variant maps to exactly one
/// of these via [`primary_entity_for`]. Adding a new kind is a wire
/// change: clients on the old vocabulary will fail to deserialise it,
/// so coordinate with a schema-version bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// A player handle — typically the local player, occasionally a
    /// remote actor surfaced through legacy `<Actor Death>` events.
    Player,
    /// A ship or ground vehicle — identified by GEID when available.
    Vehicle,
    /// An equippable item, weapon attachment, or loadout component.
    Item,
    /// A physical place — planet, moon, station, outpost, solar
    /// system. Granularity follows the event that named the place.
    Location,
    /// A shop / kiosk / commodity terminal. `id` is the shop id when
    /// the engine emitted one, otherwise a commodity-derived fallback.
    Shop,
    /// A mission instance — keyed on the engine's mission UUID.
    Mission,
    /// A play session boundary — `ProcessInit`, `JoinPu`,
    /// `ChangeServer`, `SessionEnd`. Distinct from gameplay objects.
    Session,
    /// Synthetic / out-of-game signals: HUD banners, crashes, the
    /// launcher, remote-rule matches, burst summaries.
    System,
}

/// Snake-case key for an [`EntityKind`]. Used as a cheap component of
/// `group_key` strings without allocating through `serde_json`.
pub fn entity_kind_key(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Player => "player",
        EntityKind::Vehicle => "vehicle",
        EntityKind::Item => "item",
        EntityKind::Location => "location",
        EntityKind::Shop => "shop",
        EntityKind::Mission => "mission",
        EntityKind::Session => "session",
        EntityKind::System => "system",
    }
}

/// A reference to the primary entity an event is about.
///
/// `id` is the stable identifier the timeline can dedupe / group on
/// (e.g. a player handle, a vehicle GEID, a mission UUID). It is
/// allowed to be `"unknown"` when the source line did not give us
/// one; collapsing under `"unknown"` is intentional — multiple
/// unknown-id events of the same kind should still group together
/// rather than fan out as separate rows.
///
/// `display_name` is the human-readable label the UI shows. It is
/// kept separate from `id` so the underlying identifier can be a
/// stable opaque string (a GEID, a UUID) while the label remains
/// friendly (a ship class name, a mission title).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    pub kind: EntityKind,
    pub id: String,
    pub display_name: String,
}

/// Where an event came from: did we see it directly in a log line, or
/// did we infer / synthesize it from surrounding signals?
///
/// - `Observed` — parsed straight off a real log line (or a synthetic
///   event the client produced from a directly-observed signal like a
///   crash dir). Default for everything classify currently emits.
/// - `Inferred` — the engine never wrote this event, but a downstream
///   rule deduced it from observed events (e.g. fuel-out → forced
///   spawn). Carries a provenance trail back to its source events.
/// - `Synthesized` — produced wholesale by the system without
///   observed-event ancestry (e.g. heartbeat / lifecycle markers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Observed,
    Inferred,
    Synthesized,
}

/// Per-field provenance — records, for a specific field on the event,
/// whether the value was read from the log line as-is or derived from
/// other observed events.
///
/// Serialised as an externally-tagged enum on a `type` field with
/// snake_case discriminators, so the wire form for an inferred field
/// looks like:
///
/// ```json
/// { "type": "inferred_from",
///   "source_event_ids": ["evt-1", "evt-2"],
///   "rule_id": "fuel_out_to_spawn" }
/// ```
///
/// The variant-level discriminator (not internally tagged on a
/// payload struct) keeps the JSON cheap to parse and matches the
/// existing convention used by `GameEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldProvenance {
    Observed,
    InferredFrom {
        source_event_ids: Vec<String>,
        rule_id: String,
    },
}

/// Cross-cutting metadata stamped on every event in the wire
/// envelope. See module docs for the design rationale.
///
/// `confidence` is a `f32` in `[0.0, 1.0]`. Observed events anchor at
/// `1.0`; inferred events carry a rule-supplied score; synthesized
/// events typically sit at `1.0` since they describe themselves.
///
/// `group_key` is a precomputed string the timeline uses to collapse
/// near-duplicates within a session. The format is
/// `"{event_type}:{entity_kind}:{entity_id}"` — see
/// [`group_key_for`].
///
/// The optional / map fields (`field_provenance`, `inference_inputs`,
/// `rule_id`) default to empty and are skipped at serialise time, so
/// the wire form for a plain Observed event stays as compact as
/// possible — only the four required fields appear.
///
/// `Eq` is not derived because `confidence` is an `f32`. Matches the
/// pattern already established by `GameEvent` (skips `Eq` due to
/// `AttachmentReceived.elapsed_seconds: f64`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMetadata {
    pub primary_entity: EntityRef,
    pub source: EventSource,
    pub confidence: f32,
    pub group_key: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub field_provenance: BTreeMap<String, FieldProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inference_inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

impl EventMetadata {
    /// Build metadata for an observed event — the common case. Sets
    /// `source = Observed`, `confidence = 1.0`, and leaves the
    /// optional provenance / inference fields empty.
    pub fn observed(primary_entity: EntityRef, group_key: String) -> Self {
        Self {
            primary_entity,
            source: EventSource::Observed,
            confidence: 1.0,
            group_key,
            field_provenance: BTreeMap::new(),
            inference_inputs: Vec::new(),
            rule_id: None,
        }
    }
}

/// Sentinel used when an event names an entity kind but didn't carry
/// a usable identifier (e.g. `VehicleDestruction` with no GEID). The
/// timeline still groups by `(kind, "unknown")`, which is the
/// intended behaviour — repeated unknown-id events of the same kind
/// share a row rather than fanning out.
const UNKNOWN_ID: &str = "unknown";

/// Resolve an event to its primary [`EntityRef`].
///
/// `claimed_handle` is the player handle the client claims (already
/// validated against the bearer token at the route layer). It's used
/// for events whose entity is "the local player" without the line
/// itself naming a handle — `PlayerDeath` and `PlayerIncapacitated`
/// being the canonical cases.
///
/// The mapping follows the entity table in the design spec
/// (`docs/superpowers/specs/2026-05-17-event-handling-improvements-design.md`).
pub fn primary_entity_for(event: &GameEvent, claimed_handle: Option<&str>) -> EntityRef {
    match event {
        GameEvent::PlayerDeath(_) | GameEvent::PlayerIncapacitated(_) => {
            let handle = claimed_handle.unwrap_or(UNKNOWN_ID).to_string();
            EntityRef {
                kind: EntityKind::Player,
                id: handle.clone(),
                display_name: handle,
            }
        }
        GameEvent::LegacyLogin(e) => EntityRef {
            kind: EntityKind::Player,
            id: e.handle.clone(),
            display_name: e.handle.clone(),
        },
        GameEvent::ActorDeath(e) => EntityRef {
            kind: EntityKind::Player,
            id: e.victim.clone(),
            display_name: e.victim.clone(),
        },
        GameEvent::ResolveSpawn(e) => EntityRef {
            kind: EntityKind::Player,
            id: e.player_geid.clone(),
            display_name: e.player_geid.clone(),
        },
        GameEvent::VehicleDestruction(e) => EntityRef {
            kind: EntityKind::Vehicle,
            id: e.vehicle_id.clone().unwrap_or_else(|| UNKNOWN_ID.into()),
            display_name: e.vehicle_class.clone(),
        },
        GameEvent::VehicleStowed(e) => EntityRef {
            kind: EntityKind::Vehicle,
            id: e.vehicle_id.clone(),
            display_name: e.vehicle_id.clone(),
        },
        GameEvent::QuantumTargetSelected(e) => EntityRef {
            kind: EntityKind::Vehicle,
            id: e.vehicle_id.clone(),
            display_name: e.vehicle_class.clone(),
        },
        GameEvent::AttachmentReceived(e) => EntityRef {
            kind: EntityKind::Item,
            id: e.item_id.clone(),
            display_name: e.item_class.clone(),
        },
        GameEvent::LocationInventoryRequested(e) => EntityRef {
            kind: EntityKind::Location,
            id: e.location.clone(),
            display_name: e.location.clone(),
        },
        GameEvent::PlanetTerrainLoad(e) => EntityRef {
            kind: EntityKind::Location,
            id: e.planet.clone(),
            display_name: e.planet.clone(),
        },
        GameEvent::SeedSolarSystem(e) => EntityRef {
            kind: EntityKind::Location,
            id: e.solar_system.clone(),
            display_name: e.solar_system.clone(),
        },
        GameEvent::ShopBuyRequest(e) => {
            let id = e.shop_id.clone().unwrap_or_else(|| UNKNOWN_ID.into());
            EntityRef {
                kind: EntityKind::Shop,
                id: id.clone(),
                display_name: id,
            }
        }
        GameEvent::ShopFlowResponse(e) => {
            let id = e.shop_id.clone().unwrap_or_else(|| UNKNOWN_ID.into());
            EntityRef {
                kind: EntityKind::Shop,
                id: id.clone(),
                display_name: id,
            }
        }
        GameEvent::CommodityBuyRequest(e) => {
            let id = e.commodity.clone().unwrap_or_else(|| UNKNOWN_ID.into());
            EntityRef {
                kind: EntityKind::Shop,
                id: id.clone(),
                display_name: id,
            }
        }
        GameEvent::CommoditySellRequest(e) => {
            let id = e.commodity.clone().unwrap_or_else(|| UNKNOWN_ID.into());
            EntityRef {
                kind: EntityKind::Shop,
                id: id.clone(),
                display_name: id,
            }
        }
        GameEvent::MissionStart(e) => EntityRef {
            kind: EntityKind::Mission,
            id: e.mission_id.clone(),
            display_name: e
                .mission_name
                .clone()
                .unwrap_or_else(|| e.mission_id.clone()),
        },
        GameEvent::MissionEnd(e) => {
            let id = e.mission_id.clone().unwrap_or_else(|| UNKNOWN_ID.into());
            EntityRef {
                kind: EntityKind::Mission,
                id: id.clone(),
                display_name: id,
            }
        }
        GameEvent::ProcessInit(e) => EntityRef {
            kind: EntityKind::Session,
            id: e.local_session.clone(),
            display_name: e.local_session.clone(),
        },
        GameEvent::JoinPu(e) => EntityRef {
            kind: EntityKind::Session,
            id: e.shard.clone(),
            display_name: e.shard.clone(),
        },
        GameEvent::ChangeServer(_) | GameEvent::SessionEnd(_) => EntityRef {
            kind: EntityKind::Session,
            id: "session".into(),
            display_name: "session".into(),
        },
        GameEvent::HudNotification(_) => EntityRef {
            kind: EntityKind::System,
            id: "hud".into(),
            display_name: "HUD".into(),
        },
        GameEvent::GameCrash(_) => EntityRef {
            kind: EntityKind::System,
            id: "crash".into(),
            display_name: "crash".into(),
        },
        GameEvent::LauncherActivity(_) => EntityRef {
            kind: EntityKind::System,
            id: "launcher".into(),
            display_name: "launcher".into(),
        },
        GameEvent::RemoteMatch(e) => EntityRef {
            kind: EntityKind::System,
            id: e.event_name.clone(),
            display_name: e.event_name.clone(),
        },
        GameEvent::BurstSummary(e) => EntityRef {
            kind: EntityKind::System,
            id: e.rule_id.clone(),
            display_name: e.rule_id.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{
        AttachmentReceived, GameCrash, GameEvent, LocationInventoryRequested, MissionMarkerKind,
        MissionStart, PlayerDeath, VehicleDestruction,
    };

    #[test]
    fn entity_kind_serialises_snake_case() {
        let s = serde_json::to_string(&EntityKind::Player).unwrap();
        assert_eq!(s, "\"player\"");
        let s = serde_json::to_string(&EntityKind::Vehicle).unwrap();
        assert_eq!(s, "\"vehicle\"");
        let s = serde_json::to_string(&EntityKind::Session).unwrap();
        assert_eq!(s, "\"session\"");
    }

    #[test]
    fn entity_kind_round_trips() {
        for kind in [
            EntityKind::Player,
            EntityKind::Vehicle,
            EntityKind::Item,
            EntityKind::Location,
            EntityKind::Shop,
            EntityKind::Mission,
            EntityKind::Session,
            EntityKind::System,
        ] {
            let s = serde_json::to_string(&kind).unwrap();
            let back: EntityKind = serde_json::from_str(&s).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn entity_kind_key_matches_serde_form() {
        for kind in [
            EntityKind::Player,
            EntityKind::Vehicle,
            EntityKind::Item,
            EntityKind::Location,
            EntityKind::Shop,
            EntityKind::Mission,
            EntityKind::Session,
            EntityKind::System,
        ] {
            let serde_form = serde_json::to_string(&kind).unwrap();
            // serde form is `"player"`; key is `player` without quotes.
            assert_eq!(format!("\"{}\"", entity_kind_key(kind)), serde_form);
        }
    }

    #[test]
    fn entity_ref_round_trips() {
        let r = EntityRef {
            kind: EntityKind::Player,
            id: "TheCodeSaiyan".into(),
            display_name: "TheCodeSaiyan".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: EntityRef = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
        // Confirm the JSON shape rather than just round-trip identity.
        assert!(s.contains("\"kind\":\"player\""));
        assert!(s.contains("\"id\":\"TheCodeSaiyan\""));
        assert!(s.contains("\"display_name\":\"TheCodeSaiyan\""));
    }

    #[test]
    fn event_source_serialises_snake_case() {
        assert_eq!(
            serde_json::to_string(&EventSource::Observed).unwrap(),
            "\"observed\""
        );
        assert_eq!(
            serde_json::to_string(&EventSource::Inferred).unwrap(),
            "\"inferred\""
        );
        assert_eq!(
            serde_json::to_string(&EventSource::Synthesized).unwrap(),
            "\"synthesized\""
        );
    }

    #[test]
    fn event_source_round_trips() {
        for src in [
            EventSource::Observed,
            EventSource::Inferred,
            EventSource::Synthesized,
        ] {
            let s = serde_json::to_string(&src).unwrap();
            let back: EventSource = serde_json::from_str(&s).unwrap();
            assert_eq!(src, back);
        }
    }

    #[test]
    fn field_provenance_observed_serialises_as_tagged_variant() {
        let s = serde_json::to_string(&FieldProvenance::Observed).unwrap();
        assert_eq!(s, "{\"type\":\"observed\"}");
        let back: FieldProvenance = serde_json::from_str(&s).unwrap();
        assert_eq!(back, FieldProvenance::Observed);
    }

    #[test]
    fn field_provenance_inferred_from_round_trips() {
        let p = FieldProvenance::InferredFrom {
            source_event_ids: vec!["evt-1".into(), "evt-2".into()],
            rule_id: "fuel_out_to_spawn".into(),
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"type\":\"inferred_from\""));
        assert!(s.contains("\"source_event_ids\":[\"evt-1\",\"evt-2\"]"));
        assert!(s.contains("\"rule_id\":\"fuel_out_to_spawn\""));
        let back: FieldProvenance = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn event_metadata_observed_builder_sets_defaults() {
        let m = EventMetadata::observed(
            EntityRef {
                kind: EntityKind::Player,
                id: "TheCodeSaiyan".into(),
                display_name: "TheCodeSaiyan".into(),
            },
            "player_death:player:TheCodeSaiyan".into(),
        );
        assert_eq!(m.source, EventSource::Observed);
        assert!((m.confidence - 1.0).abs() < f32::EPSILON);
        assert!(m.field_provenance.is_empty());
        assert!(m.inference_inputs.is_empty());
        assert_eq!(m.rule_id, None);
        assert_eq!(m.group_key, "player_death:player:TheCodeSaiyan");
        assert_eq!(m.primary_entity.kind, EntityKind::Player);
    }

    #[test]
    fn event_metadata_observed_omits_empty_optionals_on_wire() {
        let m = EventMetadata::observed(
            EntityRef {
                kind: EntityKind::Vehicle,
                id: "veh-1".into(),
                display_name: "MISC Freelancer".into(),
            },
            "vehicle_destruction:vehicle:veh-1".into(),
        );
        let s = serde_json::to_string(&m).unwrap();
        // skip_serializing_if must elide the three optional fields so a
        // plain Observed event stays cheap on the wire.
        assert!(!s.contains("field_provenance"));
        assert!(!s.contains("inference_inputs"));
        assert!(!s.contains("rule_id"));
        // Required fields are present.
        assert!(s.contains("\"primary_entity\""));
        assert!(s.contains("\"source\":\"observed\""));
        assert!(s.contains("\"confidence\":1.0"));
        assert!(s.contains("\"group_key\":\"vehicle_destruction:vehicle:veh-1\""));
    }

    #[test]
    fn primary_entity_for_player_death_uses_claimed_handle() {
        let ev = GameEvent::PlayerDeath(PlayerDeath {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            body_class: "body_01_noMagicPocket".into(),
            body_id: "12345".into(),
            zone: None,
        });
        let e = primary_entity_for(&ev, Some("alice"));
        assert_eq!(e.kind, EntityKind::Player);
        assert_eq!(e.id, "alice");
        assert_eq!(e.display_name, "alice");

        // Falls back to "unknown" when handle missing.
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.id, "unknown");
    }

    #[test]
    fn primary_entity_for_vehicle_destruction_uses_class_as_display() {
        let ev = GameEvent::VehicleDestruction(VehicleDestruction {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            vehicle_class: "MISC_Freelancer".into(),
            vehicle_id: Some("veh-7".into()),
            destroy_level: 2,
            caused_by: "unknown".into(),
            zone: None,
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Vehicle);
        assert_eq!(e.id, "veh-7");
        assert_eq!(e.display_name, "MISC_Freelancer");

        // Missing GEID falls back to "unknown" but keeps class as display.
        let ev2 = GameEvent::VehicleDestruction(VehicleDestruction {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            vehicle_class: "MISC_Freelancer".into(),
            vehicle_id: None,
            destroy_level: 2,
            caused_by: "unknown".into(),
            zone: None,
        });
        let e = primary_entity_for(&ev2, None);
        assert_eq!(e.id, "unknown");
        assert_eq!(e.display_name, "MISC_Freelancer");
    }

    #[test]
    fn primary_entity_for_attachment_received() {
        let ev = GameEvent::AttachmentReceived(AttachmentReceived {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            player: "alice".into(),
            item_class: "klwe_pistol_energy_lh86".into(),
            item_id: "item-42".into(),
            status: "Attached".into(),
            port: "torso".into(),
            elapsed_seconds: 2.5,
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Item);
        assert_eq!(e.id, "item-42");
        assert_eq!(e.display_name, "klwe_pistol_energy_lh86");
    }

    #[test]
    fn primary_entity_for_location_inventory_requested() {
        let ev = GameEvent::LocationInventoryRequested(LocationInventoryRequested {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            player: "alice".into(),
            location: "Stanton2_Orison".into(),
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Location);
        assert_eq!(e.id, "Stanton2_Orison");
        assert_eq!(e.display_name, "Stanton2_Orison");
    }

    #[test]
    fn primary_entity_for_mission_start_prefers_name_for_display() {
        let ev = GameEvent::MissionStart(MissionStart {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            mission_id: "uuid-abc".into(),
            marker_kind: MissionMarkerKind::Phase,
            mission_name: Some("Bounty: Septe Boutaa".into()),
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Mission);
        assert_eq!(e.id, "uuid-abc");
        assert_eq!(e.display_name, "Bounty: Septe Boutaa");

        // Falls back to id when name is missing.
        let ev2 = GameEvent::MissionStart(MissionStart {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            mission_id: "uuid-xyz".into(),
            marker_kind: MissionMarkerKind::Phase,
            mission_name: None,
        });
        let e = primary_entity_for(&ev2, None);
        assert_eq!(e.display_name, "uuid-xyz");
    }

    #[test]
    fn primary_entity_for_game_crash_uses_fixed_system_id() {
        let ev = GameEvent::GameCrash(GameCrash {
            timestamp: "2026-05-17T00:00:00.000Z".into(),
            channel: "LIVE".into(),
            crash_dir_name: "2026-05-17-00-00-00".into(),
            primary_log_name: None,
            total_size_bytes: 1024,
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::System);
        assert_eq!(e.id, "crash");
        assert_eq!(e.display_name, "crash");
    }

    #[test]
    fn event_metadata_round_trips_with_provenance() {
        let mut provenance = BTreeMap::new();
        provenance.insert(
            "zone".to_string(),
            FieldProvenance::InferredFrom {
                source_event_ids: vec!["evt-9".into()],
                rule_id: "zone_from_terrain".into(),
            },
        );
        let m = EventMetadata {
            primary_entity: EntityRef {
                kind: EntityKind::Player,
                id: "alice".into(),
                display_name: "alice".into(),
            },
            source: EventSource::Inferred,
            confidence: 0.75,
            group_key: "player_death:player:alice".into(),
            field_provenance: provenance,
            inference_inputs: vec!["evt-9".into()],
            rule_id: Some("zone_from_terrain".into()),
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: EventMetadata = serde_json::from_str(&s).unwrap();
        assert_eq!(m, back);
    }
}
