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

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
