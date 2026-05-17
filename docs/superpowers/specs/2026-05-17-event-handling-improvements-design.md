# Event Handling Improvements — Design

**Date:** 2026-05-17
**Scope:** `crates/starstats-core`, `crates/starstats-server`, `apps/tray-ui`, `apps/web`, `packages/api-client-ts`

## Problem

Four interrelated weaknesses in how the StarStats pipeline currently handles `Game.log` events:

1. **Unrecognised lines are silently dropped.** `ParseStats` (parser.rs:416–463) counts them but no submission path exists for a user to flag promising lines and feed them back to a rule author.
2. **Duplicate events flood the timeline.** The existing `BurstRule` collapse covers known floods (loadout restore, terrain churn) but unknown floods stay verbose, and there is no general "same event, same entity, ×N" collapse.
3. **The UI is type-first; the user cares about the entity.** Every variant has its own field shape. There is no canonical "impacted entity" facet to drive entity-grouped views or cross-event filters.
4. **No inference layer.** Some events are implicit — the game does not emit them but surrounding lines make them obvious (vehicle destruction → death; planet change without inventory request → location change). There is also no way to mark observed fields that were filled by inference (`PlayerDeath.zone` is already best-effort but the field carries no provenance).

Constraint the user named explicitly: **always keep all raw data so we can re-parse when new rules are published.** Collapses are visual, never lossy.

## Architecture

A single new struct, `EventMetadata`, stamped on every event in the wire envelope. The existing `GameEvent` enum is not reshaped — no variant churn, no field renames. The four features compose off the metadata:

```text
EventEnvelope {
  // existing
  idempotency_key, raw_line, event, source: LogSource, source_offset,
  // new
  metadata: EventMetadata,
}

EventMetadata {
  primary_entity:   EntityRef,
  source:           EventSource,      // Observed | Inferred | Synthesized
  confidence:       f32,              // 0.0–1.0, default 1.0 for Observed
  group_key:        String,           // hash(event_type, entity.kind, entity.id)
  field_provenance: Map<String, FieldProvenance>,
  inference_inputs: Vec<EventId>,     // empty for Observed
  rule_id:          Option<String>,   // built-in tag, remote rule id, or inference rule id
}

EntityRef { kind: EntityKind, id: String, display_name: String }
EntityKind = Player | Vehicle | Item | Location | Shop | Mission | Session | System
EventSource = Observed | Inferred | Synthesized
FieldProvenance = Observed | InferredFrom { source_event_ids: Vec<EventId>, rule_id: String }
```

### Pipeline placement

```text
Game.log line
  → parser::classify          (existing two-pass)
  → templates::detect_bursts  (existing)
  → inference::infer          (new post-classify pass)
  → metadata::stamp           (new — fills EventMetadata for every event)
  → wire::EventEnvelope       (carries metadata)

Unrecognised line
  → unknown_lines::capture    (new parallel path)
  → SQLite cache in tray
  → user reviews + submits    → POST /v1/parser-submissions
```

## Primary entity per event type

The kind enum is intentionally small. Ambiguous events get their most user-meaningful entity as the primary; the rest stay in the existing event fields and may carry a `field_provenance` marker if they were inferred.

| Kind     | Stable ID                        | Display name                  | Example events                                                       |
| -------- | -------------------------------- | ----------------------------- | -------------------------------------------------------------------- |
| Player   | handle                           | handle                        | `LegacyLogin`, `PlayerDeath`, `PlayerIncapacitated`                  |
| Vehicle  | `vehicle_id` (GEID)              | `vehicle_class`               | `VehicleDestruction`, `VehicleStowed`, `QuantumTargetSelected`       |
| Item     | `item_id`                        | `item_class`                  | `AttachmentReceived`                                                 |
| Location | `location_id` or planet OOC key  | resolved name (e.g. "Daymar") | `LocationInventoryRequested`, `PlanetTerrainLoad`, `SeedSolarSystem` |
| Shop     | `shop_id`                        | best-effort `shop_id`         | `ShopBuyRequest`, `ShopFlowResponse`                                 |
| Mission  | `mission_id`                     | `mission_name`                | `MissionStart`, `MissionEnd`                                         |
| Session  | `local_session`                  | `local_session`               | `ProcessInit`, `SessionEnd`, `JoinPu`, `ChangeServer`                |
| System   | `game` \| `launcher` \| `crash`  | sentinel                      | `GameCrash`, `LauncherActivity`, `HudNotification`                   |

For `PlayerDeath` the primary entity is the player (handle). The zone is a secondary fact carried in the existing `zone` field with `field_provenance` set if it was inferred.

## Dedupe (visual collapse)

Two events fold into one row when they share `group_key` and are **adjacent** — no event with a different `group_key` between them in the same entity section.

`group_key` is computed server-side at parse time: `hash(event_type, primary_entity.kind, primary_entity.id)`. Same key in different sessions of play does not collapse; collapse is bounded by the session. (A *session* throughout this doc means a single play session anchored by the existing `ProcessInit` → `SessionEnd` pair on `local_session`.)

Folded rows render as `<event title> ×N` with a drill-in. Drill-in expands inline to show the individual member events. Member events stay in the wire format and in storage — collapse is a render decision, not a data decision.

This sits on top of (does not replace) the existing `BurstSummary` for known floods. Burst summaries already carry their own member events in the local cache.

## Inference engine

New module `crates/starstats-core/src/inference.rs`. Pure post-classify pass over a sliding window of the most recent N events (default 200, configurable).

### Rule shape

```rust
InferenceRule {
  id: &'static str,
  trigger: TriggerPattern,                  // event shape that opens the window
  preconditions: Vec<Precondition>,         // facts to confirm from history
  window: Duration,
  consequent_followups: Vec<TriggerPattern>,
  emits: fn(&InferenceCtx) -> GameEvent,
  confidence: f32,
}
```

Rules are pure-Rust structs in v1 (append-only file, mirrored on `templates.rs`). If/when we want hot-reload, the existing `parser_defs.rs` infrastructure (ed25519, append-only manifest) is the natural extension point.

### Initial rule set

| `rule_id`                                    | Trigger                                                       | Emits                  | Confidence |
| -------------------------------------------- | ------------------------------------------------------------- | ---------------------- | ---------- |
| `implicit_death_after_vehicle_destruction`   | `VehicleDestruction` (player-piloted) + `ResolveSpawn` ≤15s   | `PlayerDeath`          | 0.85       |
| `implicit_location_change`                   | `PlanetTerrainLoad` for new planet, no intervening inventory  | `LocationChanged` (new variant) | 0.70 |
| `implicit_shop_request_timeout`              | `ShopBuyRequest` with no `ShopFlowResponse` within 30s        | `ShopRequestTimedOut` (new variant) | 0.90 |

### Supersede-by-observed reconciliation

When an observed event of the same `(event_type, primary_entity)` arrives within 5s after an inferred event, the inferred event is **superseded**. Its row is dropped from the timeline; its raw record stays in storage with `superseded_by: <observed_event_id>` for audit. No double-rows.

### Idempotency

Inferred events carry a deterministic `idempotency_key = hash(rule_id, trigger_event_id)`. Re-running the inference pass on the same input never produces duplicates.

## Unrecognised-line capture and submission

New module `crates/starstats-core/src/unknown_lines.rs` and a `unknown_lines` SQLite table in the tray's local cache.

### Capture record

```rust
UnknownLine {
  id: Uuid,
  raw_line: String,
  timestamp: Option<String>,
  shell_tag: Option<String>,
  partial_structured: BTreeMap<String, String>,
  context_before: Vec<String>,   // 5 preceding lines
  context_after: Vec<String>,    // 5 following lines (deferred until they arrive)
  game_build: String,
  channel: LogSource,
  interest_score: u8,            // 0–100
  shape_hash: String,            // normalised line shape
  occurrence_count: u32,
  first_seen: String, last_seen: String,
  detected_pii: Vec<PiiToken>,
  dismissed: bool,
}
```

### Shape normalisation

Before scoring, lines are normalised by replacing timestamps, GEIDs, UUIDs, IPs/ports, numeric IDs, and quoted strings with placeholder tokens. The hash of the shape is the dedupe key. The user sees one row per shape in the review pane.

### Interest score

| Signal                                              | Contribution |
| --------------------------------------------------- | ------------ |
| New `<ShellTag>` we have never seen                 | +40          |
| `<ShellTag>` seen but no rule matched it            | +30          |
| Contains a GEID-shaped token                        | +15          |
| Contains a known prefix (`OOC_`, `body_`, `*_class`) | +10         |
| ≥3 occurrences this session                         | +10          |
| Occurred across ≥2 sessions                         | +20          |
| Line length <20 or >2000 chars                      | −30          |
| Already matched by a `RemoteMatch`                  | excluded     |

Threshold for surfacing in the review UI: `interest_score ≥ 50`. Below threshold the line is still captured (so we can re-score later) but does not badge the UI.

### PII detection

Auto-detected token kinds: own handle, shard ID, GEID, IP/port, friend handles found in context lines. Each token gets a default redaction policy:

| Kind          | Default     |
| ------------- | ----------- |
| Own handle    | redact      |
| Shard         | redact      |
| GEID          | send as-is  |
| IP/port       | send as-is  |
| Friend handle | send as-is  |

The user can flip each token per submission.

### Submission payload

```text
POST /v1/parser-submissions
Auth: existing Bearer scheme
{
  submissions: [{
    shape_hash, raw_examples: [<≤5 redacted lines>],
    partial_structured, shell_tag,
    suggested_event_name?, suggested_field_names?, notes?,
    context_examples: [{ before, after }],
    game_build, channel,
    occurrence_count, client_anon_id,
  }, ...]
}
```

`client_anon_id` is a hashed install id (not the player handle).

### Server storage

```text
parser_submissions
  id PK, shape_hash, first_submitted_at, last_submitted_at,
  submitter_count, total_occurrence_count, payload_json,
  status ENUM(pending, drafting, rule_written, dismissed),
  reviewer_notes, rule_id FK?
```

Idempotent on `(shape_hash, client_anon_id)` — re-submitting a shape updates the count, never duplicates.

Response: `202 Accepted { accepted, deduped, ids }`. Tray marks the accepted local shapes as submitted; dismissed shapes are remembered locally so they don't re-surface next session.

Submissions are **not** auto-applied to the live parser-definitions manifest. A rule author reviews, writes the regex, and publishes through the existing flow. Once published, tray's next manifest fetch picks it up and the shape stops generating unknown entries.

## UI

### View toggle (tray + web)

- **Default: By Entity.** Entity sections sorted by last activity time desc. Section header: icon, display name, kind pill, event count, time range. Sections collapse/expand independently; state persisted per session.
- **Toggle: Chronological.** Flat timeline using an entity-first row layout: entity icon + display name as the row title, event type as a small pill.

Per-session scope. Cross-session entity views are out of scope for v1; tracked as follow-up.

### Inside a section

- Adjacent same-`group_key` events fold to `<title> ×N`. Drill-in expands the members.
- Inferred events show an "Inferred" pill plus a thin confidence bar (0–100%). Drill-in opens a side panel listing the `inference_inputs` events.
- Field-level inferred values render with an inline pill, e.g. `at Daymar [zone inferred]`.

### Submission review pane (tray only)

Side panel, slides in from the right. Lists unknown shapes sorted by `interest_score × occurrence_count`. Each row shows raw example, PII highlight with per-token toggles, optional fields for suggested event name and notes, and Submit/Dismiss actions. Web app gets no submission UI — submission needs the local unknown-line cache.

## Migration / backwards compatibility

- `EventEnvelope` schema_version bumped by one. Old clients accepted for one minor release; server synthesizes a default `EventMetadata` for events that arrive without one (`source: Observed`, `confidence: 1.0`, primary_entity derived from the event shape). After the grace release, old schema is rejected with "please upgrade".
- `packages/api-client-ts` regenerated. Both `tray-ui` and `web` pick up the new types from it.
- Already-stored events do not get retro-backfilled. Absent `field_provenance` is interpreted as "all observed".
- Feature flag `parser.enable_v2_metadata` (tray config) gates the inference pass and unknown-line capture. Default off in the release that ships the code; default on in the next.

## Testing

| Layer                                       | Coverage                                                                                                                                                                                            |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `events.rs` / `wire.rs`                     | Unit: `EventMetadata` serialisation, round-trip, defaults, schema_version handling.                                                                                                                 |
| `parser.rs`                                 | Existing classification tests extended to assert `metadata.primary_entity` per variant; new test for `RemoteMatch` entity inference.                                                                |
| `inference.rs`                              | One unit test per inference rule + a supersede-by-observed test. Property test (proptest): re-running inference on the same input is idempotent.                                                    |
| `unknown_lines.rs`                          | Shape-normalisation, interest-score, PII detection (handle, shard, GEID, IP, friend handle).                                                                                                        |
| `parser_submissions.rs` (server)            | Integration against transient SQLite: dedupe on `(shape_hash, client_anon_id)`, counter increments, idempotency of duplicate POSTs.                                                                 |
| `tray-ui` (vitest)                          | Section grouping, dedupe ×N collapse, inferred badge, field-level inference, submission pane PII toggles.                                                                                           |
| `web` (Playwright)                          | Shared timeline opens with mixed observed/inferred events; entity grouping renders; inferred badges render; no submission UI exposed.                                                               |

Fixture extension: `Game.log` test fixture gains a vehicle-destruction → resolve-spawn sequence, a planet change without inventory request, a shop buy with no flow response, and a handful of synthetic unknown lines with realistic shell tags.

## Out of scope (tracked as follow-up)

- Rule-author moderation UI for `parser_submissions` (server-side admin app).
- Cross-session entity rollup ("everything that ever happened to my Cutlass").
- Learned/auto-tuned inference confidence.
- Hot-reloadable inference rules via the remote-manifest path.
- Sidebar-style "entity rail" UI variant.

## Files affected

**New**

- `crates/starstats-core/src/inference.rs`
- `crates/starstats-core/src/unknown_lines.rs`
- `crates/starstats-core/src/metadata.rs` (entity-kind dispatch, group_key, primary_entity per variant)
- `crates/starstats-server/src/parser_submissions.rs`
- `apps/tray-ui/src/timeline/EntitySection.tsx`
- `apps/tray-ui/src/timeline/InferredBadge.tsx`
- `apps/tray-ui/src/timeline/CollapsedGroupRow.tsx`
- `apps/tray-ui/src/submissions/ReviewPane.tsx`
- `apps/tray-ui/src/submissions/PiiToggle.tsx`

**Modified**

- `crates/starstats-core/src/events.rs` (add `LocationChanged`, `ShopRequestTimedOut` variants)
- `crates/starstats-core/src/wire.rs` (`EventMetadata` on `EventEnvelope`, schema_version bump)
- `crates/starstats-core/src/parser.rs` (call `metadata::stamp` after classify; record `field_provenance` for existing best-effort fields)
- `crates/starstats-core/src/templates.rs` (preserve members for ×N drill-in)
- `crates/starstats-server/src/main.rs` (route registration)
- `packages/api-client-ts/src/types.ts` (regenerated)
- `apps/tray-ui/src/timeline/Timeline.tsx` (default to By Entity, add Chronological toggle)
- `apps/web/src/components/Timeline/*` (consume new metadata, no submission UI)
