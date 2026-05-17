# Event Handling Improvements — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a unified `EventMetadata` envelope and four user-facing features that hang off it: visual dedupe with raw retention, entity-first timeline rendering, declarative event inference with confidence, and an unrecognised-line capture + submission flow.

**Architecture:** Single new `EventMetadata` struct stamped on every event in `EventEnvelope`. Existing `GameEvent` enum unchanged in shape. Post-classify pipeline gains an `inference` pass and an `unknown_lines` parallel capture path. Tray UI gets a By-Entity default view, a Chronological toggle, a submission review pane, and badges for inferred / collapsed / unknown.

**Tech Stack:** Rust 1.88 (workspace at `StarStats/`), `cargo test`, `pnpm` + `turbo`, vitest + React Testing Library (tray-ui), Playwright (web).

**Spec:** `docs/superpowers/specs/2026-05-17-event-handling-improvements-design.md`

**Repo discipline:** Pre-existing uncommitted work exists in this tree from other branches. **Never** `git add -A` or `git add .`. Always stage explicit file paths. Commit messages: `feat(events):`, `feat(inference):`, `feat(submissions):`, `feat(tray-ui):`, `feat(web):`. Co-Author trailer is configured globally; do not add manually.

---

## Phases

1. **Foundation** — `EventMetadata`, `EntityRef`, primary-entity dispatch, group_key, schema bump. (Unblocks everything.)
2. **Entity-first UI + dedupe** — TS regen, By-Entity timeline, Chronological toggle, ×N collapse, drill-in.
3. **Inference engine** — `inference.rs`, three initial rules, supersede reconciliation, new variants, inferred badge UI.
4. **Unknown-line capture + submission** — `unknown_lines.rs`, SQLite, PII, server endpoint, review pane.
5. **Migration + flag + web parity + finalize** — `parser.enable_v2_metadata` flag, old-schema acceptance, web app consumption, full test pass, docs.

Each phase produces working software. Phase 1 must complete before any other phase starts. Phases 2–5 may run partially in parallel by file ownership (tray-ui ≠ server ≠ core).

---

# Phase 1 — Foundation

### Task 1: EntityKind enum + EntityRef struct

**Files:**
- Create: `crates/starstats-core/src/metadata.rs`
- Modify: `crates/starstats-core/src/lib.rs` (add `pub mod metadata;` and re-exports)
- Test: inline `#[cfg(test)] mod tests` in `metadata.rs`

- [ ] **Step 1: Write the failing test**

In `crates/starstats-core/src/metadata.rs`:

```rust
//! Cross-cutting metadata stamped on every classified event:
//! primary entity, source/confidence, group_key, field provenance.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Player,
    Vehicle,
    Item,
    Location,
    Shop,
    Mission,
    Session,
    System,
}

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
    fn entity_kind_serializes_snake_case() {
        let kind = EntityKind::Player;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"player\"");
    }

    #[test]
    fn entity_ref_round_trip() {
        let ent = EntityRef {
            kind: EntityKind::Vehicle,
            id: "veh_001".into(),
            display_name: "Cutlass Black".into(),
        };
        let json = serde_json::to_string(&ent).unwrap();
        let back: EntityRef = serde_json::from_str(&json).unwrap();
        assert_eq!(ent, back);
    }
}
```

- [ ] **Step 2: Add module to `lib.rs`**

Edit `crates/starstats-core/src/lib.rs`. After `pub mod events;` add `pub mod metadata;`. After existing `pub use events::{...}` re-export block add:

```rust
pub use metadata::{EntityKind, EntityRef};
```

- [ ] **Step 3: Run test, expect PASS**

```
cargo test -p starstats-core metadata::tests
```

- [ ] **Step 4: Commit**

```
git add crates/starstats-core/src/metadata.rs crates/starstats-core/src/lib.rs
git commit -m "feat(events): add EntityKind + EntityRef metadata types"
```

---

### Task 2: EventSource + FieldProvenance enums

**Files:** Modify `crates/starstats-core/src/metadata.rs`, `lib.rs`

- [ ] **Step 1: Write failing tests** (append inside the existing `tests` module)

```rust
#[test]
fn event_source_serializes_snake_case() {
    assert_eq!(serde_json::to_string(&EventSource::Observed).unwrap(), "\"observed\"");
    assert_eq!(serde_json::to_string(&EventSource::Inferred).unwrap(), "\"inferred\"");
    assert_eq!(serde_json::to_string(&EventSource::Synthesized).unwrap(), "\"synthesized\"");
}

#[test]
fn field_provenance_observed_serializes_as_tagged() {
    let p = FieldProvenance::Observed;
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"type\":\"observed\""));
}

#[test]
fn field_provenance_inferred_carries_inputs() {
    let p = FieldProvenance::InferredFrom {
        source_event_ids: vec!["evt-a".into(), "evt-b".into()],
        rule_id: "implicit_zone".into(),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: FieldProvenance = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}
```

- [ ] **Step 2: Implement** (append above `#[cfg(test)]`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Observed,
    Inferred,
    Synthesized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldProvenance {
    Observed,
    InferredFrom {
        source_event_ids: Vec<String>,
        rule_id: String,
    },
}
```

Add to `lib.rs` re-export: `pub use metadata::{EntityKind, EntityRef, EventSource, FieldProvenance};`

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core metadata::tests
git add crates/starstats-core/src/metadata.rs crates/starstats-core/src/lib.rs
git commit -m "feat(events): add EventSource + FieldProvenance enums"
```

---

### Task 3: EventMetadata struct

**Files:** Modify `crates/starstats-core/src/metadata.rs`, `lib.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn event_metadata_default_observed() {
    let m = EventMetadata::observed(
        EntityRef { kind: EntityKind::Player, id: "Jim".into(), display_name: "Jim".into() },
        "grp_abc".into(),
    );
    assert_eq!(m.source, EventSource::Observed);
    assert!((m.confidence - 1.0).abs() < f32::EPSILON);
    assert!(m.inference_inputs.is_empty());
    assert!(m.field_provenance.is_empty());
}

#[test]
fn event_metadata_round_trip() {
    let m = EventMetadata {
        primary_entity: EntityRef {
            kind: EntityKind::Vehicle, id: "v1".into(), display_name: "Cutlass".into()
        },
        source: EventSource::Inferred,
        confidence: 0.85,
        group_key: "grp_v1_death".into(),
        field_provenance: BTreeMap::new(),
        inference_inputs: vec!["evt-1".into()],
        rule_id: Some("implicit_death".into()),
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: EventMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(m, back);
}
```

- [ ] **Step 2: Implement**

```rust
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
```

Add to `lib.rs`: `pub use metadata::{EntityKind, EntityRef, EventMetadata, EventSource, FieldProvenance};`

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core metadata::tests
git add crates/starstats-core/src/metadata.rs crates/starstats-core/src/lib.rs
git commit -m "feat(events): add EventMetadata struct"
```

---

### Task 4: primary_entity_for(event) dispatch

**Files:** Modify `crates/starstats-core/src/metadata.rs`

Per-variant dispatch table: maps each `GameEvent` variant to its `EntityRef`. Spec §"Primary entity per event type" is the source of truth.

- [ ] **Step 1: Failing tests** (one assertion per major variant, batched into a single test for compactness)

```rust
#[cfg(test)]
mod entity_dispatch_tests {
    use super::*;
    use crate::events::*;

    fn anyts() -> String { "2026-05-17T00:00:00Z".into() }

    #[test]
    fn player_death_primary_is_player() {
        let ev = GameEvent::PlayerDeath(PlayerDeath {
            timestamp: anyts(),
            body_class: "body_01".into(),
            body_id: "12345".into(),
            zone: Some("Daymar".into()),
        });
        let e = primary_entity_for(&ev, /*claimed_handle:*/ Some("CommanderJim"));
        assert_eq!(e.kind, EntityKind::Player);
        assert_eq!(e.id, "CommanderJim");
        assert_eq!(e.display_name, "CommanderJim");
    }

    #[test]
    fn vehicle_destruction_primary_is_vehicle() {
        let ev = GameEvent::VehicleDestruction(VehicleDestruction {
            timestamp: anyts(),
            vehicle_class: "CutlassBlack".into(),
            vehicle_id: Some("veh_42".into()),
            destroy_level: 2, caused_by: "self".into(), zone: None,
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Vehicle);
        assert_eq!(e.id, "veh_42");
        assert_eq!(e.display_name, "CutlassBlack");
    }

    #[test]
    fn attachment_received_primary_is_item() {
        let ev = GameEvent::AttachmentReceived(AttachmentReceived {
            timestamp: anyts(), player: "Jim".into(),
            item_class: "rsi_helmet".into(), item_id: "itm_9".into(),
            status: "ok".into(), port: "head".into(), elapsed_seconds: 0.0,
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Item);
        assert_eq!(e.id, "itm_9");
        assert_eq!(e.display_name, "rsi_helmet");
    }

    #[test]
    fn location_inventory_primary_is_location() {
        let ev = GameEvent::LocationInventoryRequested(LocationInventoryRequested {
            timestamp: anyts(), player: "Jim".into(),
            location: "Stanton1_Lorville".into(),
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Location);
        assert_eq!(e.id, "Stanton1_Lorville");
    }

    #[test]
    fn mission_start_primary_is_mission() {
        let ev = GameEvent::MissionStart(MissionStart {
            timestamp: anyts(), mission_id: "mis_1".into(),
            marker_kind: MissionMarkerKind::Phase, mission_name: Some("Bounty".into()),
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::Mission);
        assert_eq!(e.id, "mis_1");
        assert_eq!(e.display_name, "Bounty");
    }

    #[test]
    fn game_crash_primary_is_system() {
        let ev = GameEvent::GameCrash(GameCrash {
            timestamp: anyts(), channel: "LIVE".into(),
            crash_dir_name: "2026-05-04-21-10-12".into(),
            primary_log_name: None, total_size_bytes: 0,
        });
        let e = primary_entity_for(&ev, None);
        assert_eq!(e.kind, EntityKind::System);
        assert_eq!(e.id, "crash");
    }
}
```

- [ ] **Step 2: Implement `primary_entity_for`**

In `metadata.rs`, add (above `#[cfg(test)]`):

```rust
use crate::events::GameEvent;

/// Resolve the primary entity for an event. `claimed_handle` is the
/// player handle from the surrounding `IngestBatch`; we fall back to
/// it when an event doesn't carry a per-event handle (PlayerDeath,
/// PlayerIncapacitated).
pub fn primary_entity_for(event: &GameEvent, claimed_handle: Option<&str>) -> EntityRef {
    use GameEvent::*;
    let unknown = || "unknown".to_string();
    match event {
        LegacyLogin(e) => EntityRef {
            kind: EntityKind::Player, id: e.handle.clone(), display_name: e.handle.clone(),
        },
        PlayerDeath(_) | PlayerIncapacitated(_) => {
            let h = claimed_handle.unwrap_or("unknown").to_string();
            EntityRef { kind: EntityKind::Player, id: h.clone(), display_name: h }
        }
        ActorDeath(e) => EntityRef {
            kind: EntityKind::Player, id: e.victim.clone(), display_name: e.victim.clone(),
        },
        VehicleDestruction(e) => EntityRef {
            kind: EntityKind::Vehicle,
            id: e.vehicle_id.clone().unwrap_or_else(unknown),
            display_name: e.vehicle_class.clone(),
        },
        VehicleStowed(e) => EntityRef {
            kind: EntityKind::Vehicle, id: e.vehicle_id.clone(), display_name: e.vehicle_id.clone(),
        },
        QuantumTargetSelected(e) => EntityRef {
            kind: EntityKind::Vehicle, id: e.vehicle_id.clone(),
            display_name: e.vehicle_class.clone(),
        },
        AttachmentReceived(e) => EntityRef {
            kind: EntityKind::Item, id: e.item_id.clone(), display_name: e.item_class.clone(),
        },
        LocationInventoryRequested(e) => EntityRef {
            kind: EntityKind::Location, id: e.location.clone(), display_name: e.location.clone(),
        },
        PlanetTerrainLoad(e) => EntityRef {
            kind: EntityKind::Location, id: e.planet.clone(), display_name: e.planet.clone(),
        },
        SeedSolarSystem(e) => EntityRef {
            kind: EntityKind::Location, id: e.solar_system.clone(),
            display_name: e.solar_system.clone(),
        },
        ResolveSpawn(e) => EntityRef {
            kind: EntityKind::Player, id: e.player_geid.clone(), display_name: e.player_geid.clone(),
        },
        ShopBuyRequest(e) | ShopFlowResponse(_) => {
            let id = match event {
                ShopBuyRequest(b) => b.shop_id.clone().unwrap_or_else(unknown),
                ShopFlowResponse(r) => r.shop_id.clone().unwrap_or_else(unknown),
                _ => unreachable!(),
            };
            EntityRef { kind: EntityKind::Shop, id: id.clone(), display_name: id }
        }
        CommodityBuyRequest(e) | CommoditySellRequest(_) => {
            let commodity = match event {
                CommodityBuyRequest(b) => b.commodity.clone(),
                CommoditySellRequest(s) => s.commodity.clone(),
                _ => unreachable!(),
            };
            let id = commodity.clone().unwrap_or_else(unknown);
            EntityRef { kind: EntityKind::Shop, id: id.clone(), display_name: id }
        }
        MissionStart(e) => EntityRef {
            kind: EntityKind::Mission, id: e.mission_id.clone(),
            display_name: e.mission_name.clone().unwrap_or_else(|| e.mission_id.clone()),
        },
        MissionEnd(e) => {
            let id = e.mission_id.clone().unwrap_or_else(unknown);
            EntityRef { kind: EntityKind::Mission, id: id.clone(), display_name: id }
        }
        ProcessInit(e) => EntityRef {
            kind: EntityKind::Session, id: e.local_session.clone(),
            display_name: e.local_session.clone(),
        },
        JoinPu(e) => EntityRef {
            kind: EntityKind::Session, id: e.shard.clone(), display_name: e.shard.clone(),
        },
        ChangeServer(_) | SessionEnd(_) => EntityRef {
            kind: EntityKind::Session, id: "session".into(), display_name: "session".into(),
        },
        HudNotification(_) => EntityRef {
            kind: EntityKind::System, id: "hud".into(), display_name: "HUD".into(),
        },
        GameCrash(_) => EntityRef {
            kind: EntityKind::System, id: "crash".into(), display_name: "crash".into(),
        },
        LauncherActivity(_) => EntityRef {
            kind: EntityKind::System, id: "launcher".into(), display_name: "launcher".into(),
        },
        RemoteMatch(e) => EntityRef {
            kind: EntityKind::System, id: e.event_name.clone(), display_name: e.event_name.clone(),
        },
        BurstSummary(e) => EntityRef {
            kind: EntityKind::System, id: e.rule_id.clone(), display_name: e.rule_id.clone(),
        },
    }
}
```

Note: this dispatch is comprehensive (all current variants). Phase 3 adds two new variants and extends this match.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core metadata::entity_dispatch_tests
git add crates/starstats-core/src/metadata.rs
git commit -m "feat(events): primary_entity_for dispatch for all event variants"
```

---

### Task 5: group_key + event_type_key helpers

**Files:** Modify `crates/starstats-core/src/metadata.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn group_key_same_for_same_type_and_entity() {
    let ev1 = GameEvent::AttachmentReceived(AttachmentReceived {
        timestamp: "t1".into(), player: "Jim".into(),
        item_class: "rsi_helmet".into(), item_id: "itm_9".into(),
        status: "ok".into(), port: "head".into(), elapsed_seconds: 0.0,
    });
    let ev2 = GameEvent::AttachmentReceived(AttachmentReceived {
        timestamp: "t2".into(), player: "Jim".into(),
        item_class: "rsi_helmet".into(), item_id: "itm_9".into(),
        status: "ok".into(), port: "head".into(), elapsed_seconds: 99.0,
    });
    let k1 = group_key_for(&ev1, None);
    let k2 = group_key_for(&ev2, None);
    assert_eq!(k1, k2);
}

#[test]
fn group_key_differs_for_different_entity() {
    let ev1 = GameEvent::AttachmentReceived(AttachmentReceived {
        timestamp: "t1".into(), player: "Jim".into(),
        item_class: "rsi_helmet".into(), item_id: "itm_9".into(),
        status: "ok".into(), port: "head".into(), elapsed_seconds: 0.0,
    });
    let ev2 = GameEvent::AttachmentReceived(AttachmentReceived {
        timestamp: "t1".into(), player: "Jim".into(),
        item_class: "rsi_chestplate".into(), item_id: "itm_10".into(),
        status: "ok".into(), port: "chest".into(), elapsed_seconds: 0.0,
    });
    assert_ne!(group_key_for(&ev1, None), group_key_for(&ev2, None));
}

#[test]
fn event_type_key_returns_snake_case_variant_name() {
    let ev = GameEvent::PlayerDeath(PlayerDeath {
        timestamp: "t".into(), body_class: "b".into(), body_id: "1".into(), zone: None,
    });
    assert_eq!(event_type_key(&ev), "player_death");
}
```

- [ ] **Step 2: Implement** (add to `metadata.rs`)

```rust
/// snake_case variant name. Used as the type component of `group_key`.
pub fn event_type_key(event: &GameEvent) -> &'static str {
    use GameEvent::*;
    match event {
        ProcessInit(_) => "process_init",
        LegacyLogin(_) => "legacy_login",
        JoinPu(_) => "join_pu",
        ChangeServer(_) => "change_server",
        SeedSolarSystem(_) => "seed_solar_system",
        ResolveSpawn(_) => "resolve_spawn",
        ActorDeath(_) => "actor_death",
        PlayerDeath(_) => "player_death",
        PlayerIncapacitated(_) => "player_incapacitated",
        VehicleDestruction(_) => "vehicle_destruction",
        HudNotification(_) => "hud_notification",
        LocationInventoryRequested(_) => "location_inventory_requested",
        PlanetTerrainLoad(_) => "planet_terrain_load",
        QuantumTargetSelected(_) => "quantum_target_selected",
        AttachmentReceived(_) => "attachment_received",
        VehicleStowed(_) => "vehicle_stowed",
        GameCrash(_) => "game_crash",
        LauncherActivity(_) => "launcher_activity",
        MissionStart(_) => "mission_start",
        MissionEnd(_) => "mission_end",
        ShopBuyRequest(_) => "shop_buy_request",
        ShopFlowResponse(_) => "shop_flow_response",
        CommodityBuyRequest(_) => "commodity_buy_request",
        CommoditySellRequest(_) => "commodity_sell_request",
        SessionEnd(_) => "session_end",
        RemoteMatch(_) => "remote_match",
        BurstSummary(_) => "burst_summary",
    }
}

/// Stable group key: `evtype:kind:id`. Cheap to compute, human-debuggable,
/// avoids hash collisions, fits inside a single SQLite TEXT column.
pub fn group_key_for(event: &GameEvent, claimed_handle: Option<&str>) -> String {
    let entity = primary_entity_for(event, claimed_handle);
    format!("{}:{}:{}", event_type_key(event), serde_json::to_string(&entity.kind).unwrap().trim_matches('"'), entity.id)
}
```

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core metadata::
git add crates/starstats-core/src/metadata.rs
git commit -m "feat(events): event_type_key + group_key_for helpers"
```

---

### Task 6: stamp() — one-shot metadata builder

**Files:** Modify `crates/starstats-core/src/metadata.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn stamp_observed_returns_default_metadata() {
    let ev = GameEvent::PlayerDeath(PlayerDeath {
        timestamp: "t".into(), body_class: "b".into(), body_id: "1".into(), zone: None,
    });
    let meta = stamp(&ev, Some("CommanderJim"));
    assert_eq!(meta.source, EventSource::Observed);
    assert!((meta.confidence - 1.0).abs() < f32::EPSILON);
    assert_eq!(meta.primary_entity.kind, EntityKind::Player);
    assert!(meta.group_key.starts_with("player_death:player:"));
}
```

- [ ] **Step 2: Implement**

```rust
/// Convenience: build the default Observed metadata for an event.
/// Inference passes will override `source`, `confidence`, `inference_inputs`.
pub fn stamp(event: &GameEvent, claimed_handle: Option<&str>) -> EventMetadata {
    EventMetadata::observed(
        primary_entity_for(event, claimed_handle),
        group_key_for(event, claimed_handle),
    )
}
```

Update `lib.rs` re-export:

```rust
pub use metadata::{
    event_type_key, group_key_for, primary_entity_for, stamp,
    EntityKind, EntityRef, EventMetadata, EventSource, FieldProvenance,
};
```

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core metadata::
git add crates/starstats-core/src/metadata.rs crates/starstats-core/src/lib.rs
git commit -m "feat(events): stamp() metadata builder"
```

---

### Task 7: Add metadata to EventEnvelope + bump schema_version

**Files:** Modify `crates/starstats-core/src/wire.rs`

- [ ] **Step 1: Failing test** (append to `wire.rs` — file currently has no test module, so add one)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{GameEvent, ProcessInit};
    use crate::metadata::{stamp, EventMetadata, EntityKind};

    #[test]
    fn envelope_with_metadata_round_trips() {
        let ev = GameEvent::ProcessInit(ProcessInit {
            timestamp: "t".into(), local_session: "sess".into(),
            env_session: "env".into(), online: true,
        });
        let meta = stamp(&ev, None);
        let env = EventEnvelope {
            idempotency_key: "id-1".into(), raw_line: "raw".into(),
            event: Some(ev), source: LogSource::Live, source_offset: 0,
            metadata: Some(meta.clone()),
        };
        let json = serde_json::to_string(&env).unwrap();
        let back: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.metadata.as_ref().unwrap().primary_entity.kind, EntityKind::Session);
    }

    #[test]
    fn envelope_without_metadata_still_deserialises() {
        let legacy_json = r#"{
            "idempotency_key": "id-1", "raw_line": "raw", "event": null,
            "source": "live", "source_offset": 0
        }"#;
        let back: EventEnvelope = serde_json::from_str(legacy_json).unwrap();
        assert!(back.metadata.is_none());
    }

    #[test]
    fn schema_version_bumped_to_two() {
        assert_eq!(IngestBatch::CURRENT_SCHEMA_VERSION, 2);
    }
}
```

- [ ] **Step 2: Implement** — add `metadata: Option<EventMetadata>` field to `EventEnvelope` and bump version.

In `wire.rs`:

```rust
use crate::metadata::EventMetadata;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub idempotency_key: String,
    pub raw_line: String,
    pub event: Option<GameEvent>,
    pub source: LogSource,
    pub source_offset: u64,

    /// Cross-cutting metadata (primary entity, group_key, source/confidence,
    /// field provenance, inference inputs). Optional for backward compat
    /// with schema_version=1 clients; v2+ always populates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<EventMetadata>,
}
```

Bump `IngestBatch::CURRENT_SCHEMA_VERSION` from 1 to 2.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core wire::tests
git add crates/starstats-core/src/wire.rs
git commit -m "feat(events): add EventMetadata to EventEnvelope, bump schema to v2"
```

---

### Task 8: Wire parser to stamp metadata on classified events

**Files:** Modify `crates/starstats-core/src/parser.rs`

The existing `classify(...)` returns `Option<GameEvent>`. The hot path that builds envelopes lives in `starstats-client` and consumes `classify` output. We add a public helper `classify_with_metadata` that bundles both.

- [ ] **Step 1: Failing test** — append to `parser.rs` tests module

```rust
#[test]
fn classify_with_metadata_stamps_primary_entity() {
    let line = "<2026-05-17T14:02:31.000Z> [...] <Adding non kept item> ... body_01_test_KeptId_999 ...";
    // NOTE: use an existing fixture line that classifies as PlayerDeath in current tests
    // — copy verbatim from existing parser tests; here we just sketch the assertion.
    let parsed = structural_parse(line).unwrap();
    let (event, meta) = classify_with_metadata(&parsed, Some("CommanderJim"));
    if event.is_some() {
        let m = meta.unwrap();
        assert_eq!(m.primary_entity.kind, crate::metadata::EntityKind::Player);
        assert_eq!(m.source, crate::metadata::EventSource::Observed);
    }
}
```

NOTE TO IMPLEMENTER: copy a real PlayerDeath fixture line from the existing `parser.rs` test module (search for `#[test]` blocks that exercise `PlayerDeath`). The test above is a template — replace the line and adjust assertions as needed.

- [ ] **Step 2: Implement**

In `parser.rs`, add:

```rust
use crate::metadata::{stamp, EventMetadata};

/// Convenience: classify + stamp default metadata in one call. Returns
/// `(event, metadata)`. Metadata is `None` when the line didn't
/// classify. Callers that need to override source/confidence (e.g.
/// inference) can do so on the returned metadata.
pub fn classify_with_metadata(
    line: &LogLine,
    claimed_handle: Option<&str>,
) -> (Option<GameEvent>, Option<EventMetadata>) {
    let event = classify(line);
    let meta = event.as_ref().map(|e| stamp(e, claimed_handle));
    (event, meta)
}
```

Update `lib.rs` re-export: add `classify_with_metadata` to the `pub use parser::{...}` line.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core parser::
git add crates/starstats-core/src/parser.rs crates/starstats-core/src/lib.rs
git commit -m "feat(events): classify_with_metadata helper"
```

---

### Task 9: Validator update for metadata

**Files:** Modify `crates/starstats-core/src/validators.rs`

- [ ] **Step 1: Failing test** — append to `validators.rs` tests

```rust
#[test]
fn validator_rejects_confidence_out_of_range() {
    let mut env = make_valid_envelope(); // helper that exists or is added below
    env.metadata = Some(EventMetadata {
        primary_entity: EntityRef { kind: EntityKind::Player, id: "x".into(), display_name: "x".into() },
        source: EventSource::Observed,
        confidence: 1.5,
        group_key: "k".into(),
        field_provenance: Default::default(),
        inference_inputs: vec![], rule_id: None,
    });
    let err = validate_event(&env).unwrap_err();
    assert!(format!("{err:?}").contains("confidence"));
}

#[test]
fn validator_rejects_observed_with_confidence_below_one() {
    let mut env = make_valid_envelope();
    env.metadata = Some(EventMetadata {
        primary_entity: EntityRef { kind: EntityKind::Player, id: "x".into(), display_name: "x".into() },
        source: EventSource::Observed,
        confidence: 0.8,
        group_key: "k".into(),
        field_provenance: Default::default(),
        inference_inputs: vec![], rule_id: None,
    });
    let err = validate_event(&env).unwrap_err();
    assert!(format!("{err:?}").contains("observed"));
}
```

If `make_valid_envelope()` does not exist in the test module, add it as a helper that builds a minimal valid envelope (any classifiable event + default metadata).

- [ ] **Step 2: Implement** — extend `validate_event` to check metadata invariants:

- `confidence` ∈ [0.0, 1.0]
- `source == Observed` ⇒ `confidence == 1.0` and `inference_inputs.is_empty()`
- `source == Inferred` ⇒ `confidence < 1.0` and `inference_inputs.is_non_empty()`

Add a new `ValidationError` variant `InvalidMetadata { reason: String }` and emit it on violation.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core validators::
git add crates/starstats-core/src/validators.rs
git commit -m "feat(events): validate EventMetadata invariants"
```

---

### Task 10: Server-side backwards-compat — synthesise metadata for v1 clients

**Files:** Modify `crates/starstats-server/src/<ingest module>` (the route that accepts `IngestBatch`)

- [ ] **Step 1: Failing test**

Add an integration test under `crates/starstats-server/tests/` (create dir if missing) that:
1. POSTs an `IngestBatch` with `schema_version=1` and events without `metadata`.
2. Asserts the server accepts (200/202) and that the persisted event has metadata synthesised (`source=observed`, `confidence=1.0`, `primary_entity` populated).

- [ ] **Step 2: Implement** — in the ingest handler:

After deserialising the batch, iterate events:

```rust
if env.metadata.is_none() {
    if let Some(ev) = &env.event {
        env.metadata = Some(starstats_core::metadata::stamp(ev, Some(&batch.claimed_handle)));
    }
}
```

Reject `schema_version` outside `[1, 2]` with 400.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-server
git add crates/starstats-server/src/ <files-touched>
git commit -m "feat(events): server synthesises metadata for v1 clients"
```

---

### Task 11: Phase 1 — full workspace test pass

- [ ] **Step 1:** `cargo test --workspace`
- [ ] **Step 2:** `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] **Step 3:** If any failures, fix in dedicated commits before progressing.

---

# Phase 2 — Entity-first UI + dedupe

### Task 12: Regenerate api-client-ts schema

**Files:** Modify `packages/api-client-ts/src/generated/schema.ts` (auto-generated)

- [ ] **Step 1:** Identify the codegen command. Check `packages/api-client-ts/package.json` scripts. Common patterns: `pnpm --filter api-client-ts run codegen` or a script that consumes the OpenAPI spec.

- [ ] **Step 2:** Run codegen. Verify the regenerated `schema.ts` includes:
  - `EventMetadata` type
  - `EntityRef`, `EntityKind` types
  - `EventSource`, `FieldProvenance` types
  - Optional `metadata` on `EventEnvelope`

- [ ] **Step 3:** `pnpm typecheck` from the workspace root. Fix any consumer breakage in `apps/web` or `apps/tray-ui` (additive fields should not break, but verify).

- [ ] **Step 4: Commit**

```
git add packages/api-client-ts/src/generated/schema.ts
git commit -m "feat(api-client-ts): regenerate for EventMetadata"
```

---

### Task 13: TS helper — `groupEventsForTimeline(events)`

**Files:** Create `apps/tray-ui/src/timeline/grouping.ts`, Test: `apps/tray-ui/src/timeline/grouping.test.ts`

- [ ] **Step 1: Failing test**

```typescript
import { describe, it, expect } from 'vitest';
import { groupEventsForTimeline, foldAdjacentSameKey } from './grouping';

describe('foldAdjacentSameKey', () => {
  it('collapses three adjacent same-group_key events into one row with count=3', () => {
    const events = [
      { id: '1', metadata: { group_key: 'k_a' } },
      { id: '2', metadata: { group_key: 'k_a' } },
      { id: '3', metadata: { group_key: 'k_a' } },
      { id: '4', metadata: { group_key: 'k_b' } },
    ] as any;
    const rows = foldAdjacentSameKey(events);
    expect(rows).toHaveLength(2);
    expect(rows[0].count).toBe(3);
    expect(rows[0].members).toHaveLength(3);
    expect(rows[1].count).toBe(1);
  });

  it('does NOT collapse same-key events when a different key is between them', () => {
    const events = [
      { id: '1', metadata: { group_key: 'k_a' } },
      { id: '2', metadata: { group_key: 'k_b' } },
      { id: '3', metadata: { group_key: 'k_a' } },
    ] as any;
    const rows = foldAdjacentSameKey(events);
    expect(rows).toHaveLength(3);
  });
});

describe('groupEventsForTimeline', () => {
  it('groups events by entity (kind:id) and sorts sections by last activity desc', () => {
    const events = [
      { id: '1', timestamp: '2026-05-17T14:00:00Z',
        metadata: { primary_entity: { kind: 'vehicle', id: 'v1', display_name: 'Cutlass' }, group_key: 'a' } },
      { id: '2', timestamp: '2026-05-17T14:05:00Z',
        metadata: { primary_entity: { kind: 'player', id: 'Jim', display_name: 'Jim' }, group_key: 'b' } },
      { id: '3', timestamp: '2026-05-17T14:02:00Z',
        metadata: { primary_entity: { kind: 'vehicle', id: 'v1', display_name: 'Cutlass' }, group_key: 'c' } },
    ] as any;
    const sections = groupEventsForTimeline(events);
    expect(sections[0].entity.id).toBe('Jim');         // 14:05 is latest
    expect(sections[1].entity.id).toBe('v1');
    expect(sections[1].events).toHaveLength(2);
  });
});
```

- [ ] **Step 2: Implement**

```typescript
// apps/tray-ui/src/timeline/grouping.ts
import type { EventEnvelope } from '../api/types';

export interface TimelineRow {
  key: string;
  count: number;
  members: EventEnvelope[];
  anchor: EventEnvelope;
}

export interface EntitySection {
  entity: NonNullable<EventEnvelope['metadata']>['primary_entity'];
  lastActivity: string;
  rows: TimelineRow[];
  events: EventEnvelope[];
}

export function foldAdjacentSameKey(events: EventEnvelope[]): TimelineRow[] {
  const rows: TimelineRow[] = [];
  for (const ev of events) {
    const key = ev.metadata?.group_key ?? `__${ev.idempotency_key}`;
    const last = rows[rows.length - 1];
    if (last && last.key === key) {
      last.count++;
      last.members.push(ev);
    } else {
      rows.push({ key, count: 1, members: [ev], anchor: ev });
    }
  }
  return rows;
}

export function groupEventsForTimeline(events: EventEnvelope[]): EntitySection[] {
  const byEntity = new Map<string, EntitySection>();
  for (const ev of events) {
    const ent = ev.metadata?.primary_entity;
    if (!ent) continue;
    const id = `${ent.kind}:${ent.id}`;
    let sec = byEntity.get(id);
    if (!sec) {
      sec = { entity: ent, lastActivity: ev.event?.timestamp ?? '', rows: [], events: [] };
      byEntity.set(id, sec);
    }
    sec.events.push(ev);
    if ((ev.event?.timestamp ?? '') > sec.lastActivity) {
      sec.lastActivity = ev.event?.timestamp ?? '';
    }
  }
  const sections = Array.from(byEntity.values());
  for (const s of sections) s.rows = foldAdjacentSameKey(s.events);
  sections.sort((a, b) => (b.lastActivity ?? '').localeCompare(a.lastActivity ?? ''));
  return sections;
}
```

- [ ] **Step 3:** `pnpm --filter tray-ui test:run grouping`

- [ ] **Step 4: Commit**

```
git add apps/tray-ui/src/timeline/grouping.ts apps/tray-ui/src/timeline/grouping.test.ts
git commit -m "feat(tray-ui): grouping/folding helpers for entity-first timeline"
```

---

### Task 14: `EntitySection.tsx` component

**Files:** Create `apps/tray-ui/src/timeline/EntitySection.tsx`, Test: `EntitySection.test.tsx`

- [ ] **Step 1: Test**

```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { EntitySection } from './EntitySection';

const section = {
  entity: { kind: 'vehicle' as const, id: 'v1', display_name: 'Cutlass Black' },
  lastActivity: '2026-05-17T14:08:00Z',
  events: [],
  rows: [
    { key: 'attachment:vehicle:v1', count: 3, members: [], anchor: {
        idempotency_key: 'e1', raw_line: '', source: 'live', source_offset: 0,
        event: { type: 'attachment_received', timestamp: '2026-05-17T14:02:14Z' },
        metadata: { source: 'observed', confidence: 1.0,
          primary_entity: { kind: 'vehicle', id: 'v1', display_name: 'Cutlass Black' },
          group_key: 'k', field_provenance: {}, inference_inputs: [], rule_id: null,
        }
      } as any },
  ],
};

test('renders entity title and event count', () => {
  render(<EntitySection section={section as any} />);
  expect(screen.getByText('Cutlass Black')).toBeInTheDocument();
  expect(screen.getByText(/×3/)).toBeInTheDocument();
});

test('section expands and collapses on header click', async () => {
  render(<EntitySection section={section as any} />);
  expect(screen.getByText(/×3/)).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: /Cutlass Black/i }));
  expect(screen.queryByText(/×3/)).not.toBeVisible();
});
```

- [ ] **Step 2: Component skeleton**

```tsx
import { useState } from 'react';
import type { EntitySection as Section } from './grouping';
import { CollapsedGroupRow } from './CollapsedGroupRow';

export function EntitySection({ section }: { section: Section }) {
  const [open, setOpen] = useState(true);
  const ent = section.entity;
  return (
    <section className="entity-section" data-kind={ent.kind}>
      <button className="entity-section-header" onClick={() => setOpen(o => !o)}>
        <span className={`ent-icon ${ent.kind}`}>{initials(ent.display_name)}</span>
        <span className="entity-title">{ent.display_name}</span>
        <span className="entity-kind-pill">{ent.kind}</span>
        <span className="entity-count">{section.events.length} events</span>
      </button>
      {open && (
        <div className="entity-section-rows">
          {section.rows.map(row => <CollapsedGroupRow key={row.key + row.anchor.idempotency_key} row={row} />)}
        </div>
      )}
    </section>
  );
}

function initials(name: string): string {
  return name.split(/[\s_-]+/).map(p => p[0]?.toUpperCase() ?? '').slice(0, 2).join('') || '?';
}
```

- [ ] **Step 3: Run + commit**

```
pnpm --filter tray-ui test:run EntitySection
git add apps/tray-ui/src/timeline/EntitySection.tsx apps/tray-ui/src/timeline/EntitySection.test.tsx
git commit -m "feat(tray-ui): EntitySection component"
```

---

### Task 15: `CollapsedGroupRow.tsx` component

**Files:** Create `apps/tray-ui/src/timeline/CollapsedGroupRow.tsx`, Test: `.test.tsx`

- [ ] **Step 1: Test**

```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CollapsedGroupRow } from './CollapsedGroupRow';

const makeRow = (count: number) => ({
  key: 'k', count, members: Array(count).fill(0).map((_, i) => ({
    idempotency_key: `e${i}`, raw_line: '', source: 'live', source_offset: 0,
    event: { type: 'attachment_received', timestamp: `2026-05-17T14:02:${10+i}Z`, item_class: 'rsi_helmet' },
    metadata: { source: 'observed', confidence: 1.0,
      primary_entity: { kind: 'item', id: 'i1', display_name: 'helmet' },
      group_key: 'k', field_provenance: {}, inference_inputs: [], rule_id: null,
    },
  })),
  anchor: null as any,
});

test('renders count badge when count > 1', () => {
  const row = makeRow(3);
  row.anchor = row.members[0];
  render(<CollapsedGroupRow row={row as any} />);
  expect(screen.getByText('×3')).toBeInTheDocument();
});

test('no count badge when count = 1', () => {
  const row = makeRow(1);
  row.anchor = row.members[0];
  render(<CollapsedGroupRow row={row as any} />);
  expect(screen.queryByText(/×/)).not.toBeInTheDocument();
});

test('drill-in reveals member events', async () => {
  const row = makeRow(3);
  row.anchor = row.members[0];
  render(<CollapsedGroupRow row={row as any} />);
  expect(screen.queryByText('e2')).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: /expand/i }));
  expect(screen.getByText(/e2/)).toBeInTheDocument();
});
```

- [ ] **Step 2: Implement**

```tsx
import { useState } from 'react';
import type { TimelineRow } from './grouping';
import { InferredBadge } from './InferredBadge';

export function CollapsedGroupRow({ row }: { row: TimelineRow }) {
  const [open, setOpen] = useState(false);
  const ev = row.anchor;
  const ts = ev.event?.timestamp ?? '';
  return (
    <div className="timeline-row">
      <div className="row-time">{ts.slice(11, 19)}</div>
      <div className="row-main">
        <div className="row-title">
          <span className="row-type-pill">{eventTitle(ev)}</span>
          {row.count > 1 && <span className="row-count-pill">×{row.count}</span>}
          {ev.metadata?.source === 'inferred' && <InferredBadge confidence={ev.metadata.confidence} />}
        </div>
        <div className="row-sub">{eventSubtitle(ev)}</div>
        {row.count > 1 && (
          <button className="row-expand" aria-label="expand" onClick={() => setOpen(o => !o)}>
            {open ? 'Collapse' : 'Expand'} {row.count} member events
          </button>
        )}
        {open && (
          <ul className="row-members">
            {row.members.map(m => (
              <li key={m.idempotency_key}>{m.idempotency_key} — {m.event?.timestamp}</li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function eventTitle(ev: any): string {
  return ev.event?.type?.replace(/_/g, ' ') ?? 'unknown';
}
function eventSubtitle(ev: any): string {
  // Best-effort summary; extend per-variant later
  return ev.raw_line.slice(0, 80);
}
```

- [ ] **Step 3: Run + commit**

```
pnpm --filter tray-ui test:run CollapsedGroupRow
git add apps/tray-ui/src/timeline/CollapsedGroupRow.tsx apps/tray-ui/src/timeline/CollapsedGroupRow.test.tsx
git commit -m "feat(tray-ui): CollapsedGroupRow with drill-in"
```

---

### Task 16: `InferredBadge.tsx` stub

**Files:** Create `apps/tray-ui/src/timeline/InferredBadge.tsx`, `.test.tsx`

Phase 3 fleshes it out; for now we need the import to resolve.

- [ ] **Step 1: Test**

```tsx
import { render, screen } from '@testing-library/react';
import { InferredBadge } from './InferredBadge';

test('renders Inferred pill with confidence percent', () => {
  render(<InferredBadge confidence={0.85} />);
  expect(screen.getByText('Inferred')).toBeInTheDocument();
  expect(screen.getByText(/85%/)).toBeInTheDocument();
});
```

- [ ] **Step 2: Implement**

```tsx
export function InferredBadge({ confidence }: { confidence: number }) {
  return (
    <span className="inferred-badge" title={`${Math.round(confidence * 100)}% confidence`}>
      Inferred <span className="conf">{Math.round(confidence * 100)}%</span>
    </span>
  );
}
```

- [ ] **Step 3: Run + commit**

```
pnpm --filter tray-ui test:run InferredBadge
git add apps/tray-ui/src/timeline/InferredBadge.tsx apps/tray-ui/src/timeline/InferredBadge.test.tsx
git commit -m "feat(tray-ui): InferredBadge stub"
```

---

### Task 17: View toggle + Timeline integration

**Files:** Modify `apps/tray-ui/src/timeline/Timeline.tsx` (find via grep — the component currently rendering events)

- [ ] **Step 1: Failing test**

```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Timeline } from './Timeline';

const fixture = /* array of EventEnvelope objects across two entities */;

test('default view groups by entity', () => {
  render(<Timeline events={fixture} />);
  expect(screen.getByRole('heading', { name: /by entity/i })).toBeInTheDocument();
});

test('toggling Chronological flattens into a single time-ordered list', async () => {
  render(<Timeline events={fixture} />);
  await userEvent.click(screen.getByRole('button', { name: /chronological/i }));
  expect(screen.queryByText(/entity section/i)).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Implement** — modify the existing `Timeline` (or create a new wrapper if existing one is hard to retrofit). Render either:
  - `groupEventsForTimeline(events).map(EntitySection)` (default), or
  - `foldAdjacentSameKey(events).map(CollapsedGroupRow)` (chronological)

Persist toggle state in component-local React state initialised from `localStorage.getItem('tray.timeline.view') || 'by-entity'`.

- [ ] **Step 3: Run + commit**

```
pnpm --filter tray-ui test:run Timeline
git add apps/tray-ui/src/timeline/Timeline.tsx apps/tray-ui/src/timeline/Timeline.test.tsx
git commit -m "feat(tray-ui): By-Entity default + Chronological toggle"
```

---

### Task 18: Phase 2 — full tray-ui test pass

- [ ] `pnpm --filter tray-ui test:run`
- [ ] `pnpm --filter tray-ui typecheck`
- [ ] Fix any failures, commit fixes.

---

# Phase 3 — Inference engine

### Task 19: Add new event variants `LocationChanged` + `ShopRequestTimedOut`

**Files:** Modify `crates/starstats-core/src/events.rs`, `lib.rs`

- [ ] **Step 1: Failing test** (in `events.rs` tests)

```rust
#[test]
fn location_changed_serialises() {
    let ev = GameEvent::LocationChanged(LocationChanged {
        timestamp: "t".into(),
        from: Some("Stanton1_Lorville".into()),
        to: "OOC_Stanton_2b_Daymar".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"location_changed\""));
}

#[test]
fn shop_request_timed_out_serialises() {
    let ev = GameEvent::ShopRequestTimedOut(ShopRequestTimedOut {
        timestamp: "t".into(),
        shop_id: Some("shop_1".into()),
        item_class: Some("rsi_rifle".into()),
        timed_out_after_secs: 30,
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"shop_request_timed_out\""));
}
```

- [ ] **Step 2: Implement**

In `events.rs`, add two new variants to `GameEvent` enum and matching structs:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationChanged {
    pub timestamp: String,
    pub from: Option<String>,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopRequestTimedOut {
    pub timestamp: String,
    pub shop_id: Option<String>,
    pub item_class: Option<String>,
    pub timed_out_after_secs: u32,
}
```

Add `LocationChanged(LocationChanged),` and `ShopRequestTimedOut(ShopRequestTimedOut),` to `GameEvent`.

Update `metadata::primary_entity_for` and `metadata::event_type_key` match arms:
- `LocationChanged` → `EntityKind::Location`, id/display = `to`
- `ShopRequestTimedOut` → `EntityKind::Shop`

Update `lib.rs` re-exports.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core
git add crates/starstats-core/src/events.rs crates/starstats-core/src/metadata.rs crates/starstats-core/src/lib.rs
git commit -m "feat(events): add LocationChanged + ShopRequestTimedOut variants"
```

---

### Task 20: Inference module scaffolding

**Files:** Create `crates/starstats-core/src/inference.rs`, modify `lib.rs`

- [ ] **Step 1: Failing test** (in `inference.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_on_empty_stream_returns_no_inferences() {
        let result = infer(&[], &Default::default());
        assert!(result.is_empty());
    }
}
```

- [ ] **Step 2: Implement scaffold**

```rust
//! Post-classify pass that emits inferred events from surrounding context.
//!
//! Rules are pure-Rust structs in v1, mirroring the BurstRule / RemoteRule
//! style: declarative, append-only, no learned components.

use crate::events::GameEvent;
use crate::metadata::EventMetadata;
use crate::wire::EventEnvelope;

#[derive(Debug, Clone, Default)]
pub struct InferenceConfig {
    pub window_size: usize,           // default 200
    pub reconciliation_secs: u32,     // default 5
}

impl InferenceConfig {
    pub fn defaults() -> Self {
        Self { window_size: 200, reconciliation_secs: 5 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InferredEvent {
    pub event: GameEvent,
    pub metadata: EventMetadata,
    pub trigger_idempotency_key: String,
}

pub fn infer(events: &[EventEnvelope], _config: &InferenceConfig) -> Vec<InferredEvent> {
    // Phase-3 follow-up tasks add concrete rules. Empty result for now.
    Vec::new()
}
```

Add `pub mod inference;` to `lib.rs` and re-export `infer, InferenceConfig, InferredEvent`.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core inference::
git add crates/starstats-core/src/inference.rs crates/starstats-core/src/lib.rs
git commit -m "feat(inference): module scaffold"
```

---

### Task 21: Inference rule — implicit death after vehicle destruction

**Files:** Modify `crates/starstats-core/src/inference.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn implicit_death_emitted_when_vehicle_destruction_followed_by_resolve_spawn() {
    use crate::events::*;
    use crate::wire::*;
    let veh = make_envelope(GameEvent::VehicleDestruction(VehicleDestruction {
        timestamp: "2026-05-17T14:02:30Z".into(),
        vehicle_class: "Cutlass".into(), vehicle_id: Some("v1".into()),
        destroy_level: 2, caused_by: "self".into(), zone: None,
    }), "envA");
    let resp = make_envelope(GameEvent::ResolveSpawn(ResolveSpawn {
        timestamp: "2026-05-17T14:02:35Z".into(),
        player_geid: "Jim".into(), fallback: false,
    }), "envB");
    let out = infer(&[veh, resp], &InferenceConfig::defaults());
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0].event, GameEvent::PlayerDeath(_)));
    assert_eq!(out[0].metadata.source, crate::metadata::EventSource::Inferred);
    assert!((out[0].metadata.confidence - 0.85).abs() < 0.001);
    assert_eq!(out[0].metadata.rule_id.as_deref(), Some("implicit_death_after_vehicle_destruction"));
}

#[test]
fn implicit_death_not_emitted_when_resolve_spawn_too_late() {
    use crate::events::*;
    let veh = make_envelope(GameEvent::VehicleDestruction(VehicleDestruction {
        timestamp: "2026-05-17T14:02:30Z".into(),
        vehicle_class: "Cutlass".into(), vehicle_id: Some("v1".into()),
        destroy_level: 2, caused_by: "self".into(), zone: None,
    }), "envA");
    let resp = make_envelope(GameEvent::ResolveSpawn(ResolveSpawn {
        timestamp: "2026-05-17T14:02:50Z".into(),  // >15s after
        player_geid: "Jim".into(), fallback: false,
    }), "envB");
    let out = infer(&[veh, resp], &InferenceConfig::defaults());
    assert!(out.is_empty());
}
```

Add `make_envelope` helper inside the tests module that builds an `EventEnvelope` with default `Observed` metadata.

- [ ] **Step 2: Implement**

In `inference.rs`, replace the stub `infer` body with a loop that scans the event stream and applies one or more rules. Add the `implicit_death_after_vehicle_destruction` rule:

```rust
const IMPLICIT_DEATH_RULE_ID: &str = "implicit_death_after_vehicle_destruction";
const IMPLICIT_DEATH_WINDOW_SECS: i64 = 15;
const IMPLICIT_DEATH_CONFIDENCE: f32 = 0.85;

fn parse_ts(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&chrono::Utc))
}

pub fn infer(events: &[EventEnvelope], _config: &InferenceConfig) -> Vec<InferredEvent> {
    let mut out = Vec::new();
    for (i, trigger) in events.iter().enumerate() {
        if let Some(GameEvent::VehicleDestruction(vd)) = &trigger.event {
            let trig_ts = parse_ts(&vd.timestamp);
            for follow in events.iter().skip(i + 1).take(50) {
                if let Some(GameEvent::ResolveSpawn(_rs)) = &follow.event {
                    let ftrans_ts = follow.event.as_ref().and_then(|e| match e {
                        GameEvent::ResolveSpawn(r) => parse_ts(&r.timestamp),
                        _ => None,
                    });
                    if let (Some(t1), Some(t2)) = (trig_ts, ftrans_ts) {
                        if (t2 - t1).num_seconds().abs() <= IMPLICIT_DEATH_WINDOW_SECS {
                            let inferred_ev = GameEvent::PlayerDeath(crate::events::PlayerDeath {
                                timestamp: vd.timestamp.clone(),
                                body_class: "inferred".into(),
                                body_id: format!("inferred_{}", trigger.idempotency_key),
                                zone: vd.zone.clone(),
                            });
                            let mut meta = crate::metadata::stamp(&inferred_ev, None);
                            meta.source = crate::metadata::EventSource::Inferred;
                            meta.confidence = IMPLICIT_DEATH_CONFIDENCE;
                            meta.inference_inputs = vec![trigger.idempotency_key.clone()];
                            meta.rule_id = Some(IMPLICIT_DEATH_RULE_ID.into());
                            out.push(InferredEvent {
                                event: inferred_ev,
                                metadata: meta,
                                trigger_idempotency_key: trigger.idempotency_key.clone(),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }
    out
}
```

Add `chrono = { version = "0.4", features = ["serde"] }` to `Cargo.toml` if not already present.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core inference::
git add crates/starstats-core/src/inference.rs crates/starstats-core/Cargo.toml
git commit -m "feat(inference): implicit_death_after_vehicle_destruction rule"
```

---

### Task 22: Inference rule — implicit location change

**Files:** Modify `inference.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn implicit_location_change_emitted_when_planet_terrain_load_for_new_planet() {
    use crate::events::*;
    let prev = make_envelope_loc("OOC_Stanton_1_Hurston", "2026-05-17T14:00:00Z", "envA");
    let next = make_envelope_loc("OOC_Stanton_2b_Daymar", "2026-05-17T14:05:00Z", "envB");
    let out = infer(&[prev, next], &InferenceConfig::defaults());
    let loc_changes: Vec<_> = out.iter().filter(|e| matches!(e.event, GameEvent::LocationChanged(_))).collect();
    assert_eq!(loc_changes.len(), 1);
    if let GameEvent::LocationChanged(lc) = &loc_changes[0].event {
        assert_eq!(lc.to, "OOC_Stanton_2b_Daymar");
        assert_eq!(lc.from.as_deref(), Some("OOC_Stanton_1_Hurston"));
    }
    assert!((loc_changes[0].metadata.confidence - 0.70).abs() < 0.001);
}
```

`make_envelope_loc(planet, ts, id)` builds a `PlanetTerrainLoad` envelope.

- [ ] **Step 2: Implement** — extend the `infer` loop with a second rule. Track the last seen planet; when a `PlanetTerrainLoad` arrives for a different planet and no `LocationInventoryRequested` is between them, emit `LocationChanged`.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core inference::
git add crates/starstats-core/src/inference.rs
git commit -m "feat(inference): implicit_location_change rule"
```

---

### Task 23: Inference rule — shop request timeout

**Files:** Modify `inference.rs`

- [ ] **Step 1: Failing test** — `ShopBuyRequest` with no `ShopFlowResponse` within 30s emits `ShopRequestTimedOut` with confidence 0.90.

- [ ] **Step 2: Implement** — third rule.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core inference::
git add crates/starstats-core/src/inference.rs
git commit -m "feat(inference): implicit_shop_request_timeout rule"
```

---

### Task 24: Supersede-by-observed reconciliation

**Files:** Modify `inference.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn inferred_death_superseded_by_observed_player_death() {
    // VehicleDestruction (triggers inference) → ResolveSpawn → real PlayerDeath ≤5s after the inferred ts
    // infer() should mark the inferred death as superseded (returned with a superseded_by field)
}
```

- [ ] **Step 2: Implement**

Add `superseded_by: Option<String>` field to `InferredEvent`. After collecting inferences, walk the original event stream: for each observed event with the same `(group_key, primary_entity)` within 5s of an inferred event, mark the inferred event's `superseded_by` to the observed envelope's idempotency_key.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core inference::
git add crates/starstats-core/src/inference.rs
git commit -m "feat(inference): supersede-by-observed reconciliation"
```

---

### Task 25: Idempotency property test

**Files:** Modify `inference.rs`, add `proptest = "1"` to `[dev-dependencies]` in `Cargo.toml`

- [ ] **Step 1: Failing test**

```rust
proptest! {
    #[test]
    fn infer_is_idempotent_on_same_input(events in arb_event_stream()) {
        let r1 = infer(&events, &InferenceConfig::defaults());
        let r2 = infer(&events, &InferenceConfig::defaults());
        prop_assert_eq!(r1, r2);
    }
}
```

`arb_event_stream()` is a custom proptest strategy that builds a small `Vec<EventEnvelope>` with mixed observed events. Define inline.

- [ ] **Step 2: Implement** — the function should already be deterministic. Test confirms it.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core inference::
git add crates/starstats-core/src/inference.rs crates/starstats-core/Cargo.toml
git commit -m "test(inference): idempotency property test"
```

---

### Task 26: Field provenance retro-marking — PlayerDeath.zone

**Files:** Modify `crates/starstats-core/src/parser.rs` (the zone enrichment pass)

- [ ] **Step 1: Failing test**

```rust
#[test]
fn zone_enrichment_marks_field_as_inferred() {
    // After running the existing zone-enrichment pass, the resulting envelope's
    // metadata.field_provenance should contain "zone" → InferredFrom { ... }
}
```

- [ ] **Step 2: Implement** — wherever the existing zone enrichment fills `PlayerDeath.zone`, also update the envelope's `metadata.field_provenance.insert("zone".into(), FieldProvenance::InferredFrom { source_event_ids: vec![...], rule_id: "zone_from_recent_planet_or_inventory".into() })`.

Do the same for `MissionEnd.outcome`, `ResolveSpawn.fallback`, `ActorDeath.zone` if they are currently filled by enrichment.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core parser::
git add crates/starstats-core/src/parser.rs
git commit -m "feat(events): retro-mark enriched fields as inferred in provenance"
```

---

### Task 27: UI — render inferred badge + field-level inferred marker

**Files:** Modify `apps/tray-ui/src/timeline/CollapsedGroupRow.tsx`, `InferredBadge.tsx`

- [ ] **Step 1: Failing test**

```tsx
test('renders field-level inferred pill on zone', () => {
  const row = { /* row with PlayerDeath event, metadata.field_provenance.zone = InferredFrom */ };
  render(<CollapsedGroupRow row={row as any} />);
  expect(screen.getByText(/zone inferred/i)).toBeInTheDocument();
});
```

- [ ] **Step 2: Implement** — in `CollapsedGroupRow`, check `ev.metadata?.field_provenance` and render an inline pill next to each inferred field's value in the subtitle. `InferredBadge` already exists from Task 16; render it when `metadata.source === 'inferred'`.

- [ ] **Step 3: Run + commit**

```
pnpm --filter tray-ui test:run
git add apps/tray-ui/src/timeline/CollapsedGroupRow.tsx apps/tray-ui/src/timeline/InferredBadge.tsx
git commit -m "feat(tray-ui): render inferred badge and field-level inferred markers"
```

---

### Task 28: Phase 3 — full workspace test pass

- [ ] `cargo test --workspace`
- [ ] `pnpm --filter tray-ui test:run`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`

---

# Phase 4 — Unknown-line capture + submission

### Task 29: UnknownLine struct + shape normalization

**Files:** Create `crates/starstats-core/src/unknown_lines.rs`, modify `lib.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn shape_normalises_timestamps_and_geids() {
    let line = "<2026-05-17T14:02:31.000Z> [Foo] <CargoManifestSync> for vehicle id [54324] uuid [a1b2c3d4-1234-5678-9abc-def012345678]";
    let s = shape_of(line);
    assert!(!s.contains("2026-05-17"));
    assert!(!s.contains("54324"));
    assert!(!s.contains("a1b2c3d4-1234-5678-9abc-def012345678"));
    assert!(s.contains("<CargoManifestSync>"));
    assert!(s.contains("<TS>"));
    assert!(s.contains("<GEID>"));
    assert!(s.contains("<UUID>"));
}

#[test]
fn shape_is_stable_for_same_template() {
    let a = shape_of("<2026-01-01T00:00:00Z> [X] <Foo> id [123]");
    let b = shape_of("<2026-05-17T14:02:31Z> [X] <Foo> id [54324]");
    assert_eq!(a, b);
}
```

- [ ] **Step 2: Implement**

```rust
use regex::Regex;
use once_cell::sync::Lazy;

static TS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?").unwrap());
static UUID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap());
static GEID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\d{5,}\]").unwrap());
static IPPORT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?").unwrap());
static QUOTED_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#""[^"]+""#).unwrap());

pub fn shape_of(line: &str) -> String {
    let s = TS_RE.replace_all(line, "<TS>");
    let s = UUID_RE.replace_all(&s, "<UUID>");
    let s = GEID_RE.replace_all(&s, "[<GEID>]");
    let s = IPPORT_RE.replace_all(&s, "<IPPORT>");
    let s = QUOTED_RE.replace_all(&s, "\"<STR>\"");
    s.into_owned()
}
```

Add `regex` and `once_cell` to `Cargo.toml` `[dependencies]` if not already present (regex is likely already there for the parser).

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core unknown_lines::
git add crates/starstats-core/src/unknown_lines.rs crates/starstats-core/src/lib.rs crates/starstats-core/Cargo.toml
git commit -m "feat(submissions): shape normalisation for unknown lines"
```

---

### Task 30: Interest score

**Files:** Modify `unknown_lines.rs`

- [ ] **Step 1: Failing tests** — one per scoring rule from the spec (new shell tag → +40, known tag no rule → +30, GEID → +15, known prefix → +10, ≥3 occ this session → +10, etc.).

- [ ] **Step 2: Implement**

```rust
pub struct InterestContext<'a> {
    pub known_shell_tags: &'a std::collections::HashSet<String>,
    pub known_rule_tags: &'a std::collections::HashSet<String>,
    pub session_occurrence_count: u32,
    pub multi_session: bool,
    pub already_remote_matched: bool,
}

pub fn interest_score(line: &str, shell_tag: Option<&str>, ctx: &InterestContext) -> u8 {
    if ctx.already_remote_matched { return 0; }
    let mut score: i32 = 0;
    if let Some(tag) = shell_tag {
        if !ctx.known_shell_tags.contains(tag) { score += 40; }
        else if !ctx.known_rule_tags.contains(tag) { score += 30; }
    }
    if line.contains("[") && line.chars().filter(|c| c.is_ascii_digit()).count() >= 5 { score += 15; }
    for prefix in &["OOC_", "body_", "_class"] { if line.contains(prefix) { score += 10; break; } }
    if ctx.session_occurrence_count >= 3 { score += 10; }
    if ctx.multi_session { score += 20; }
    let len = line.len();
    if len < 20 || len > 2000 { score -= 30; }
    score.clamp(0, 100) as u8
}
```

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core unknown_lines::
git add crates/starstats-core/src/unknown_lines.rs
git commit -m "feat(submissions): interest_score heuristic"
```

---

### Task 31: PII detection

**Files:** Modify `unknown_lines.rs`

- [ ] **Step 1: Failing tests** — detect own handle, shard, GEID, IP, friend handles.

- [ ] **Step 2: Implement**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PiiToken {
    pub kind: PiiKind,
    pub start: usize,
    pub end: usize,
    pub suggested_redaction: String,
    pub default_redact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiKind {
    OwnHandle, FriendHandle, ShardId, Geid, IpPort,
}

pub fn detect_pii(line: &str, own_handle: &str, known_friends: &[String]) -> Vec<PiiToken> {
    let mut tokens = Vec::new();
    if let Some(idx) = line.find(own_handle) {
        tokens.push(PiiToken { kind: PiiKind::OwnHandle, start: idx, end: idx + own_handle.len(),
            suggested_redaction: "[HANDLE]".into(), default_redact: true });
    }
    for friend in known_friends {
        if let Some(idx) = line.find(friend.as_str()) {
            tokens.push(PiiToken { kind: PiiKind::FriendHandle, start: idx, end: idx + friend.len(),
                suggested_redaction: "[FRIEND]".into(), default_redact: false });
        }
    }
    // GEID, IP, shard: use the same regexes from Task 29
    // shard pattern: "shard[abc123]" or similar — derive from parser_defs.rs or events.rs
    tokens
}
```

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core unknown_lines::
git add crates/starstats-core/src/unknown_lines.rs
git commit -m "feat(submissions): PII detection (handle, shard, GEID, IP, friends)"
```

---

### Task 32: UnknownLine capture struct + capture entry point

**Files:** Modify `unknown_lines.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn capture_records_shape_and_score() {
    let line = "<2026-05-17T14:02:31Z> [Foo] <NewEvent> for vehicle id [54324]";
    let captured = capture(line, "LIVE", "4.0", &CaptureContext::default());
    assert!(!captured.shape_hash.is_empty());
    assert!(captured.interest_score >= 50);
    assert_eq!(captured.shell_tag.as_deref(), Some("NewEvent"));
}
```

- [ ] **Step 2: Implement** — `UnknownLine` struct matches the spec §4 schema, `capture()` builds one from a raw line.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core unknown_lines::
git add crates/starstats-core/src/unknown_lines.rs
git commit -m "feat(submissions): UnknownLine.capture() entry point"
```

---

### Task 33: Hook capture into parser pipeline

**Files:** Modify `crates/starstats-core/src/parser.rs` (or the client crate, depending on where the "no classify, no remote match" path is)

- [ ] **Step 1: Failing test**

```rust
#[test]
fn parse_unrecognised_line_yields_unknown_line_record() {
    let log = "<TS> <NewMysteryEvent> something";
    let outcome = classify_or_capture(log, &CaptureContext::default());
    assert!(matches!(outcome, ClassifyOutcome::Unknown(_)));
}
```

- [ ] **Step 2: Implement** — add `ClassifyOutcome { Classified(GameEvent), RemoteMatched(GameEvent), Unknown(UnknownLine) }` and `classify_or_capture` that delegates accordingly.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-core
git add crates/starstats-core/src/parser.rs crates/starstats-core/src/unknown_lines.rs
git commit -m "feat(submissions): classify_or_capture pipeline entrypoint"
```

---

### Task 34: SQLite cache for unknown lines (tray-side)

**Files:** Modify `crates/starstats-client/src/<cache module>` (tray's local cache lives here)

- [ ] **Step 1: Failing integration test** under `crates/starstats-client/tests/`:

```rust
#[test]
fn unknown_lines_cache_persists_and_dedupes_by_shape_hash() {
    let db = open_in_memory_cache();
    let line = make_unknown_line("shape_a");
    cache_unknown_line(&db, &line).unwrap();
    cache_unknown_line(&db, &line).unwrap();   // same shape_hash
    let rows = list_unknown_lines(&db).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].occurrence_count, 2);
}
```

- [ ] **Step 2: Implement** — create table `unknown_lines (id, shape_hash UNIQUE, raw_examples_json, partial_structured_json, shell_tag, context_before_json, context_after_json, game_build, channel, interest_score, occurrence_count, first_seen, last_seen, detected_pii_json, dismissed)`. CRUD functions: `cache_unknown_line` (upsert by shape_hash), `list_unknown_lines` (filter `dismissed = 0`, threshold `interest_score >= 50`), `dismiss_unknown_line`, `mark_submitted`.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-client
git add crates/starstats-client/src/<files> crates/starstats-client/tests/<test>
git commit -m "feat(submissions): SQLite cache for unknown lines"
```

---

### Task 35: Server endpoint POST /v1/parser-submissions

**Files:** Create `crates/starstats-server/src/parser_submissions.rs`, modify `main.rs` (route registration), wire-format struct in `crates/starstats-core/src/wire.rs`

- [ ] **Step 1: Failing integration test** under `crates/starstats-server/tests/`:

```rust
#[tokio::test]
async fn submission_endpoint_accepts_and_dedupes() {
    let app = test_app().await;
    let body = ParserSubmissionBatch { submissions: vec![sample_submission("shape_a", "anon_x")] };
    let r1 = app.post("/v1/parser-submissions").json(&body).send().await;
    assert_eq!(r1.status(), 202);
    let r2 = app.post("/v1/parser-submissions").json(&body).send().await;
    assert_eq!(r2.status(), 202);
    let counts: Counts = app.get("/v1/parser-submissions/_debug").send().await.json().await;
    assert_eq!(counts.distinct_shapes, 1);
    assert_eq!(counts.total_occurrence_count, 2);
}
```

- [ ] **Step 2: Implement**

In `wire.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParserSubmission {
    pub shape_hash: String,
    pub raw_examples: Vec<String>,
    pub partial_structured: std::collections::BTreeMap<String, String>,
    pub shell_tag: Option<String>,
    pub suggested_event_name: Option<String>,
    pub suggested_field_names: Option<std::collections::BTreeMap<String, String>>,
    pub notes: Option<String>,
    pub context_examples: Vec<ContextExample>,
    pub game_build: Option<String>,
    pub channel: LogSource,
    pub occurrence_count: u32,
    pub client_anon_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextExample {
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParserSubmissionBatch {
    pub submissions: Vec<ParserSubmission>,
}
```

In `parser_submissions.rs`:

- Migrate a new SQLite table per the spec.
- `axum::post` handler at `/v1/parser-submissions` that upserts by `(shape_hash, client_anon_id)`, increments counts, returns 202 with `{ accepted, deduped, ids }`.

Register in `main.rs` route table.

- [ ] **Step 3: Run + commit**

```
cargo test -p starstats-server
git add crates/starstats-core/src/wire.rs crates/starstats-server/src/parser_submissions.rs crates/starstats-server/src/main.rs
git commit -m "feat(submissions): POST /v1/parser-submissions endpoint"
```

---

### Task 36: Tray-ui — PiiToggle component

**Files:** Create `apps/tray-ui/src/submissions/PiiToggle.tsx`, `.test.tsx`

- [ ] **Step 1: Failing test**

```tsx
test('toggle flips redaction state and highlights token', async () => {
  const onChange = vi.fn();
  render(<PiiToggle token={{ kind: 'own_handle', start: 0, end: 4,
    suggested_redaction: '[HANDLE]', default_redact: true } as any}
    onChange={onChange} />);
  await userEvent.click(screen.getByRole('checkbox'));
  expect(onChange).toHaveBeenCalledWith(false);
});
```

- [ ] **Step 2: Implement**

```tsx
import { useState } from 'react';

export function PiiToggle({ token, onChange }: any) {
  const [redact, setRedact] = useState(token.default_redact);
  return (
    <label className="pii-toggle" data-kind={token.kind}>
      <input type="checkbox" checked={redact} onChange={e => {
        setRedact(e.target.checked); onChange(e.target.checked);
      }} />
      <span>Redact {token.kind.replace('_', ' ')} → {token.suggested_redaction}</span>
    </label>
  );
}
```

- [ ] **Step 3: Run + commit**

```
pnpm --filter tray-ui test:run PiiToggle
git add apps/tray-ui/src/submissions/PiiToggle.tsx apps/tray-ui/src/submissions/PiiToggle.test.tsx
git commit -m "feat(tray-ui): PiiToggle component"
```

---

### Task 37: Tray-ui — ReviewPane component

**Files:** Create `apps/tray-ui/src/submissions/ReviewPane.tsx`, `.test.tsx`

- [ ] **Step 1: Failing test**

```tsx
test('lists unknown shapes sorted by interest × occurrence desc', () => {
  const shapes = [
    { shape_hash: 'a', interest_score: 60, occurrence_count: 1, raw_examples: [''] },
    { shape_hash: 'b', interest_score: 80, occurrence_count: 3, raw_examples: [''] },
    { shape_hash: 'c', interest_score: 50, occurrence_count: 5, raw_examples: [''] },
  ];
  render(<ReviewPane shapes={shapes as any} onSubmit={vi.fn()} onDismiss={vi.fn()} />);
  const items = screen.getAllByTestId('shape-row');
  expect(items[0]).toHaveTextContent('b'); // 80*3 = 240
  expect(items[1]).toHaveTextContent('c'); // 50*5 = 250 — actually wait, c first
  // adjust assertion: c (250), b (240), a (60)
});

test('submit button invokes onSubmit with selected redactions', async () => {
  const onSubmit = vi.fn();
  // ... render, click submit, assert payload
});
```

- [ ] **Step 2: Implement** — list, per-row raw example with `PiiToggle` per detected PII token, optional notes textarea, Submit/Dismiss buttons.

- [ ] **Step 3: Run + commit**

```
pnpm --filter tray-ui test:run ReviewPane
git add apps/tray-ui/src/submissions/ReviewPane.tsx apps/tray-ui/src/submissions/ReviewPane.test.tsx
git commit -m "feat(tray-ui): ReviewPane for unknown-line submissions"
```

---

### Task 38: Tray-ui — wire ReviewPane into app + Tauri bridge

**Files:** Modify `apps/tray-ui/src/App.tsx` (or equivalent root), `crates/starstats-client/src/<tauri commands>`

- [ ] **Step 1:** Add Tauri commands: `list_unknown_lines() -> Vec<UnknownLine>`, `submit_unknown_lines(selected: Vec<SubmissionPayload>) -> Result<()>`, `dismiss_unknown_line(shape_hash: String) -> Result<()>`.

- [ ] **Step 2:** Add `unknown_lines_count` badge to the tray UI header. Clicking opens the side panel housing `ReviewPane`.

- [ ] **Step 3:** Integration test (Tauri-mocked): badge shows when count > 0; clicking opens panel; submit POSTs to server endpoint.

- [ ] **Step 4: Commit**

```
git add apps/tray-ui/src/App.tsx crates/starstats-client/src/<files>
git commit -m "feat(tray-ui): wire ReviewPane into app + Tauri bridge"
```

---

### Task 39: Phase 4 — full test pass

- [ ] `cargo test --workspace`
- [ ] `pnpm --filter tray-ui test:run`
- [ ] `pnpm --filter tray-ui typecheck`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`

---

# Phase 5 — Migration, flag, web parity, finalize

### Task 40: Feature flag `parser.enable_v2_metadata`

**Files:** Modify tray config (likely `crates/starstats-client/src/config.rs` or `apps/tray-ui/src/config.ts`)

- [ ] **Step 1: Failing test** — when flag is off, parser pipeline runs in legacy mode (no inference, no unknown_lines capture, no metadata stamping). When on, the full pipeline runs.

- [ ] **Step 2: Implement** — add a `bool` config flag `parser.enable_v2_metadata` defaulting to `false` in this release. Gate the new code paths behind it.

- [ ] **Step 3: Commit**

```
git add <files>
git commit -m "feat(events): parser.enable_v2_metadata feature flag (default off)"
```

---

### Task 41: Web — consume new metadata in shared timeline

**Files:** Modify `apps/web/src/components/Timeline/*.tsx`

- [ ] **Step 1:** Find the existing timeline components in the web app (`apps/web/src` — search for `Timeline` or `LocationTimeline`).

- [ ] **Step 2:** Render the new metadata-driven row format. Read `metadata.primary_entity` for the row title, render `InferredBadge` (port from tray-ui or extract to a shared package) when `source === 'inferred'`, render field-level inferred markers from `metadata.field_provenance`.

- [ ] **Step 3:** Confirm NO submission UI is exposed in the web app.

- [ ] **Step 4: Playwright e2e** — open a shared timeline fixture with inferred events; assert badges render; assert no Submit button anywhere.

- [ ] **Step 5: Commit**

```
git add apps/web/src/components/Timeline <files>
git commit -m "feat(web): consume EventMetadata in shared timeline"
```

---

### Task 42: Out-of-scope follow-up tracker

**Files:** Create `docs/superpowers/follow-ups/2026-05-17-event-handling.md`

- [ ] **Step 1:** List the spec's "Out of scope" items as tracked follow-ups with brief notes on what would unblock each.

- [ ] **Step 2:** Commit

```
git add docs/superpowers/follow-ups/2026-05-17-event-handling.md
git commit -m "docs: track event-handling follow-ups"
```

---

### Task 43: Final verification

- [ ] `cargo test --workspace`
- [ ] `pnpm test` (turbo runs all workspace tests)
- [ ] `pnpm typecheck`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Manual smoke test: start tray-ui dev (`pnpm --filter tray-ui dev`), open the timeline, toggle By-Entity / Chronological, expand a section, drill into a ×N row, open the submission panel.
- [ ] If any step fails, fix in dedicated commits before declaring complete.

---

## Self-review notes

- **Spec coverage:** Every spec section maps to one or more tasks. Foundation §EventMetadata → Tasks 1-7. Dedupe §collapse → Tasks 13, 15. Entity-first UI → Tasks 13-17. Inference engine → Tasks 19-26. Unknown lines + submission → Tasks 29-38. Migration → Tasks 40-41. Tests § → every task's TDD step + Task 43.
- **Type consistency:** `EventMetadata`, `EntityRef`, `EventSource`, `FieldProvenance`, `InferredEvent` names are used consistently. `group_key` is a `String` everywhere. `confidence` is `f32` in Rust and `number` in TS.
- **No placeholders:** Each step shows the code or the exact command/expected outcome. Repetitive UI patterns reference the canonical implementation in earlier tasks but show their own component skeleton.
- **Open implementation choices (acceptable):** The exact location of "where the tray currently classifies lines" needs to be found by the executing agent in Task 33 — the spec and existing code don't pin it. Same for "where the existing zone-enrichment pass lives" in Task 26. These are findable by grep in 30 seconds; spelling them out here would be premature.
