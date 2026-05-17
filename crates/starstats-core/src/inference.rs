//! Post-classify pass that emits inferred events from surrounding context.
//!
//! Rules are pure-Rust structs in v1, mirroring the BurstRule / RemoteRule
//! style: declarative, append-only, no learned components. The inference
//! pass is pure — given the same input event slice it produces the same
//! output, which keeps tests deterministic and idempotent.

use crate::events::{GameEvent, PlayerDeath};
use crate::metadata::{
    entity_kind_key, event_type_key, primary_entity_for, EntityRef, EventMetadata, EventSource,
};
use crate::wire::EventEnvelope;
use chrono::{DateTime, FixedOffset};
use std::collections::BTreeMap;

/// Tunables for the inference pass. The defaults match the design spec
/// (Phase 3); callers needing tighter bounds (e.g. unit tests probing
/// edge cases) can override.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    /// Hard cap on the forward / backward scan distance in events. Acts
    /// as a defence-in-depth bound on top of the per-rule wall-clock
    /// limits so a malformed log can't make a rule walk the whole stream.
    pub window_size: usize,
    /// Reconciliation window in seconds. An inferred event is marked
    /// `superseded_by` the first observed event of matching
    /// `(event_type, primary_entity)` that lands within this window
    /// after the inferred timestamp.
    pub reconciliation_secs: i64,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            window_size: 200,
            reconciliation_secs: 5,
        }
    }
}

/// One result row from [`infer`]. Carries the synthesised event, its
/// metadata, and a back-reference to the observed event whose
/// classification triggered the rule.
#[derive(Debug, Clone, PartialEq)]
pub struct InferredEvent {
    pub event: GameEvent,
    pub metadata: EventMetadata,
    /// The observed event whose classification triggered this inference.
    /// Used for idempotency_key derivation and audit trail.
    pub trigger_idempotency_key: String,
    /// If this inferred event was later superseded by an actual observed
    /// event of the same `(event_type, primary_entity)` within the
    /// reconciliation window, this is the observed event's idempotency_key.
    /// Timeline consumers drop superseded rows but storage retains them.
    pub superseded_by: Option<String>,
}

/// Run all inference rules over the event stream. Pure function:
/// given the same input it returns the same output.
pub fn infer(events: &[EventEnvelope], config: &InferenceConfig) -> Vec<InferredEvent> {
    let mut out = Vec::new();
    rule_implicit_death_after_vehicle_destruction(events, config, &mut out);
    out
}

/// Parse the wire timestamps as fixed-offset RFC3339. Returns `None`
/// when the field doesn't parse — rules treat that as "skip this
/// pairing" rather than failing the whole pass.
fn parse_ts(ts: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(ts).ok()
}

/// Extract the parsed game event's timestamp, when the envelope
/// actually carries a classified event.
fn envelope_timestamp(env: &EventEnvelope) -> Option<&str> {
    let ev = env.event.as_ref()?;
    Some(match ev {
        GameEvent::ProcessInit(e) => &e.timestamp,
        GameEvent::LegacyLogin(e) => &e.timestamp,
        GameEvent::JoinPu(e) => &e.timestamp,
        GameEvent::ChangeServer(e) => &e.timestamp,
        GameEvent::SeedSolarSystem(e) => &e.timestamp,
        GameEvent::ResolveSpawn(e) => &e.timestamp,
        GameEvent::ActorDeath(e) => &e.timestamp,
        GameEvent::PlayerDeath(e) => &e.timestamp,
        GameEvent::PlayerIncapacitated(e) => &e.timestamp,
        GameEvent::VehicleDestruction(e) => &e.timestamp,
        GameEvent::HudNotification(e) => &e.timestamp,
        GameEvent::LocationInventoryRequested(e) => &e.timestamp,
        GameEvent::PlanetTerrainLoad(e) => &e.timestamp,
        GameEvent::QuantumTargetSelected(e) => &e.timestamp,
        GameEvent::AttachmentReceived(e) => &e.timestamp,
        GameEvent::VehicleStowed(e) => &e.timestamp,
        GameEvent::GameCrash(e) => &e.timestamp,
        GameEvent::LauncherActivity(e) => &e.timestamp,
        GameEvent::MissionStart(e) => &e.timestamp,
        GameEvent::MissionEnd(e) => &e.timestamp,
        GameEvent::ShopBuyRequest(e) => &e.timestamp,
        GameEvent::ShopFlowResponse(e) => &e.timestamp,
        GameEvent::CommodityBuyRequest(e) => &e.timestamp,
        GameEvent::CommoditySellRequest(e) => &e.timestamp,
        GameEvent::SessionEnd(e) => &e.timestamp,
        GameEvent::RemoteMatch(e) => &e.timestamp,
        GameEvent::BurstSummary(e) => &e.timestamp,
        GameEvent::LocationChanged(e) => &e.timestamp,
        GameEvent::ShopRequestTimedOut(e) => &e.timestamp,
    })
}

/// Build the inferred-event metadata block. Centralised so every rule
/// produces a consistent envelope (snake_case group_key shape, Inferred
/// source, sub-1.0 confidence) without duplicating the format string.
fn build_inferred_metadata(
    primary_entity: EntityRef,
    event: &GameEvent,
    confidence: f32,
    rule_id: &str,
    inference_inputs: Vec<String>,
) -> EventMetadata {
    let group_key = format!(
        "{}:{}:{}",
        event_type_key(event),
        entity_kind_key(primary_entity.kind),
        primary_entity.id,
    );
    EventMetadata {
        primary_entity,
        source: EventSource::Inferred,
        confidence,
        group_key,
        field_provenance: BTreeMap::new(),
        inference_inputs,
        rule_id: Some(rule_id.to_string()),
    }
}

/// Rule: `implicit_death_after_vehicle_destruction`.
///
/// Pattern: a `VehicleDestruction` immediately followed (within 15s
/// and `window_size` events) by a `ResolveSpawn` is the engine's tell
/// that the local player died with their ship. The line classifier
/// doesn't get a `<Actor Death>` for the pilot in modern builds (CIG
/// removed that branch), so we synthesise the death here at the
/// VehicleDestruction's timestamp.
///
/// The rule emits a `PlayerDeath` with `body_class = "inferred"` and a
/// derived `body_id` so consumers can distinguish a real corpse-cleanup
/// from this synthesised marker. Confidence is 0.85 — high but not
/// 1.0, because the engine occasionally writes spawn-resolve lines
/// for non-death scenarios (e.g. an explicit respawn beacon).
const RULE_ID_IMPLICIT_DEATH: &str = "implicit_death_after_vehicle_destruction";
const IMPLICIT_DEATH_WINDOW_SECS: i64 = 15;
const IMPLICIT_DEATH_CONFIDENCE: f32 = 0.85;

fn rule_implicit_death_after_vehicle_destruction(
    events: &[EventEnvelope],
    config: &InferenceConfig,
    out: &mut Vec<InferredEvent>,
) {
    for (i, env) in events.iter().enumerate() {
        let Some(GameEvent::VehicleDestruction(veh)) = env.event.as_ref() else {
            continue;
        };
        let Some(veh_ts) = parse_ts(&veh.timestamp) else {
            continue;
        };

        // Scan forward within the bounded window for a ResolveSpawn
        // landing within the 15s wall-clock budget.
        let scan_end = (i + 1 + config.window_size).min(events.len());
        let mut matched_spawn: Option<&EventEnvelope> = None;
        for follower in &events[(i + 1)..scan_end] {
            let Some(follower_ts) = envelope_timestamp(follower).and_then(parse_ts) else {
                continue;
            };
            let delta = (follower_ts - veh_ts).num_seconds();
            if delta > IMPLICIT_DEATH_WINDOW_SECS {
                break;
            }
            if delta < 0 {
                continue;
            }
            if matches!(follower.event.as_ref(), Some(GameEvent::ResolveSpawn(_))) {
                matched_spawn = Some(follower);
                break;
            }
        }

        let Some(spawn) = matched_spawn else {
            continue;
        };

        let inferred = PlayerDeath {
            timestamp: veh.timestamp.clone(),
            body_class: "inferred".into(),
            body_id: format!("inferred_{}", env.idempotency_key),
            zone: veh.zone.clone(),
        };
        let event = GameEvent::PlayerDeath(inferred);
        // Primary entity follows the same dispatch the wire format
        // uses; falls back to "unknown" when no claimed handle is in
        // scope for the pure inference pass.
        let primary_entity = primary_entity_for(&event, None);
        let metadata = build_inferred_metadata(
            primary_entity,
            &event,
            IMPLICIT_DEATH_CONFIDENCE,
            RULE_ID_IMPLICIT_DEATH,
            vec![env.idempotency_key.clone(), spawn.idempotency_key.clone()],
        );
        out.push(InferredEvent {
            event,
            metadata,
            trigger_idempotency_key: env.idempotency_key.clone(),
            superseded_by: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{ResolveSpawn, VehicleDestruction};
    use crate::wire::LogSource;

    fn make_envelope(event: GameEvent, idk: &str) -> EventEnvelope {
        EventEnvelope {
            idempotency_key: idk.into(),
            raw_line: format!("synthetic_for_{idk}"),
            event: Some(event),
            source: LogSource::Live,
            source_offset: 0,
            metadata: None,
        }
    }

    #[test]
    fn infer_on_empty_stream_returns_no_inferences() {
        assert!(infer(&[], &InferenceConfig::default()).is_empty());
    }

    #[test]
    fn implicit_death_emitted_when_vehicle_destruction_followed_by_resolve_spawn() {
        let veh = make_envelope(
            GameEvent::VehicleDestruction(VehicleDestruction {
                timestamp: "2026-05-17T14:02:30Z".into(),
                vehicle_class: "Cutlass".into(),
                vehicle_id: Some("v1".into()),
                destroy_level: 2,
                caused_by: "self".into(),
                zone: None,
            }),
            "envA",
        );
        let resp = make_envelope(
            GameEvent::ResolveSpawn(ResolveSpawn {
                timestamp: "2026-05-17T14:02:35Z".into(),
                player_geid: "Jim".into(),
                fallback: false,
            }),
            "envB",
        );
        let out = infer(&[veh, resp], &InferenceConfig::default());
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].event, GameEvent::PlayerDeath(_)));
        assert_eq!(out[0].metadata.source, EventSource::Inferred);
        assert!((out[0].metadata.confidence - 0.85).abs() < 0.001);
        assert_eq!(
            out[0].metadata.rule_id.as_deref(),
            Some("implicit_death_after_vehicle_destruction")
        );
        assert_eq!(out[0].metadata.inference_inputs.len(), 2);
        assert_eq!(out[0].trigger_idempotency_key, "envA");
    }

    #[test]
    fn implicit_death_not_emitted_when_resolve_spawn_too_late() {
        let veh = make_envelope(
            GameEvent::VehicleDestruction(VehicleDestruction {
                timestamp: "2026-05-17T14:02:30Z".into(),
                vehicle_class: "Cutlass".into(),
                vehicle_id: Some("v1".into()),
                destroy_level: 2,
                caused_by: "self".into(),
                zone: None,
            }),
            "envA",
        );
        let resp = make_envelope(
            GameEvent::ResolveSpawn(ResolveSpawn {
                timestamp: "2026-05-17T14:02:50Z".into(),
                player_geid: "Jim".into(),
                fallback: false,
            }),
            "envB",
        );
        assert!(infer(&[veh, resp], &InferenceConfig::default()).is_empty());
    }

    #[test]
    fn implicit_death_not_emitted_when_no_resolve_spawn() {
        let veh = make_envelope(
            GameEvent::VehicleDestruction(VehicleDestruction {
                timestamp: "2026-05-17T14:02:30Z".into(),
                vehicle_class: "Cutlass".into(),
                vehicle_id: Some("v1".into()),
                destroy_level: 2,
                caused_by: "self".into(),
                zone: None,
            }),
            "envA",
        );
        assert!(infer(&[veh], &InferenceConfig::default()).is_empty());
    }
}
