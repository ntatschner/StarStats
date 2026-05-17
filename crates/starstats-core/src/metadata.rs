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
}
