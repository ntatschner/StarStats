//! Strongly-typed gameplay events parsed from `Game.log`.
//!
//! Each variant captures *only* fields we have evidence for in real
//! captures. Adding a new variant should be paired with a corresponding
//! tag-based dispatch in `parser::classify`.

use serde::{Deserialize, Serialize};

/// Top-level event enum. Tagged representation so it round-trips
/// cleanly through JSON for the wire format.
///
/// Note: this can't derive `Eq` because `AttachmentReceived` stores
/// `elapsed_seconds: f64`, which only implements `PartialEq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameEvent {
    ProcessInit(ProcessInit),
    LegacyLogin(LegacyLogin),
    JoinPu(JoinPu),
    ChangeServer(ChangeServer),
    SeedSolarSystem(SeedSolarSystem),
    ResolveSpawn(ResolveSpawn),
    ActorDeath(ActorDeath),
    PlayerDeath(PlayerDeath),
    PlayerIncapacitated(PlayerIncapacitated),
    VehicleDestruction(VehicleDestruction),
    HudNotification(HudNotification),
    LocationInventoryRequested(LocationInventoryRequested),
    PlanetTerrainLoad(PlanetTerrainLoad),
    QuantumTargetSelected(QuantumTargetSelected),
    AttachmentReceived(AttachmentReceived),
    VehicleStowed(VehicleStowed),
    GameCrash(GameCrash),
    LauncherActivity(LauncherActivity),
    MissionStart(MissionStart),
    MissionEnd(MissionEnd),
    ShopBuyRequest(ShopBuyRequest),
    ShopFlowResponse(ShopFlowResponse),
    CommodityBuyRequest(CommodityBuyRequest),
    CommoditySellRequest(CommoditySellRequest),
    SessionEnd(SessionEnd),
    RemoteMatch(RemoteMatch),
    BurstSummary(BurstSummary),
}

/// `<Init> Process sc-client started` — anchors the start of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInit {
    pub timestamp: String,
    pub local_session: String,
    pub env_session: String,
    pub online: bool,
}

/// `<Legacy login response> ... Handle[X] - Time[Y]` — gives us the
/// authoritative player handle for the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyLogin {
    pub timestamp: String,
    pub handle: String,
    pub server_time: Option<String>,
}

/// `<Join PU> address[X] port[Y] shard[Z] locationId[W]` — actual
/// transition into a Persistent Universe shard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinPu {
    pub timestamp: String,
    pub address: String,
    pub port: u16,
    pub shard: String,
    pub location_id: String,
}

/// `<Change Server Start>` / `<Change Server End>` — server transitions.
/// Consolidated into one event with a `phase`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeServer {
    pub timestamp: String,
    pub phase: ServerPhase,
    pub is_shard_persisted: bool,
    pub is_server: bool,
    pub is_multiplayer: bool,
    pub is_online: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerPhase {
    Start,
    End,
}

/// `<Seed Solar System> ... in Solar System 'X' for shard Y`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedSolarSystem {
    pub timestamp: String,
    pub solar_system: String,
    pub shard: String,
    pub success: bool,
}

/// `<ResolveSpawnLocation Location Not Found> ... player id: [GEID]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveSpawn {
    pub timestamp: String,
    pub player_geid: String,
    pub fallback: bool,
}

/// `<Adding non kept item [CSCActorCorpseUtils::PopulateItemPortForItemRecoveryEntitlement]>`
/// filtered to the player's body line — the canonical "you died" event
/// in modern (4.x+) Star Citizen builds.
///
/// CIG removed the explicit `<Actor Death>` event with attribution at
/// some point pre-4.x. The remaining death signal is the corpse-cleanup
/// burst that starts with the player's `body_*` actor item being marked
/// for inventory-recovery entitlement; subsequent lines in the burst
/// are the loadout items (armor, weapons, mags) and aren't classified
/// individually — only the first `body_*` line counts as a death.
///
/// Only the local player's deaths are written to Game.log this way;
/// NPC corpses don't go through `ItemRecoveryEntitlement`. So a match
/// here means "I died" without ambiguity.
///
/// `zone` is `None` at classify time. A future enrichment pass walks
/// recent `PlanetTerrainLoad` / `LocationInventoryRequested` events to
/// fill it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerDeath {
    pub timestamp: String,
    /// e.g. `body_01_noMagicPocket` — the body class without the
    /// trailing instance ID.
    pub body_class: String,
    /// Trailing instance ID — same value also appears as `KeptId` on
    /// the line. Useful for de-duping if the same line somehow lands
    /// twice.
    pub body_id: String,
    /// Best-effort location-of-death string, derived post-classify by
    /// scanning recent zone-relevant events. None at parse time.
    pub zone: Option<String>,
}

/// `<SHUDEvent_OnNotification>` with body text starting with
/// "Incapacitated:" — the survivable downed state. Emitted instead of
/// the generic `HudNotification` so callers can distinguish it
/// without parsing the banner text. Distinct from `PlayerDeath`:
/// players can be revived from incapacitation, but if the
/// "Time to Death" timer expires a `PlayerDeath` follows ~30s later.
///
/// `zone` is None at classify time, same enrichment story as
/// `PlayerDeath`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerIncapacitated {
    pub timestamp: String,
    /// The notification queue id the engine appends in `[NN]`. Useful
    /// for correlating with the matching `<UpdateNotificationItem>`
    /// removal line if we ever care about how long the user spent
    /// incapacitated.
    pub queue_id: u64,
    pub zone: Option<String>,
}

/// `<Actor Death>` — legacy combat kill / NPC death event. CIG no
/// longer writes lines in this format in modern builds; the parser
/// is kept against older log captures and the synthetic fixture in
/// the unit tests so historical data still classifies. Live deaths
/// flow through `PlayerDeath` instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorDeath {
    pub timestamp: String,
    pub victim: String,
    pub victim_geid: Option<String>,
    pub zone: String,
    pub killer: String,
    pub killer_geid: Option<String>,
    pub weapon: String,
    pub damage_type: String,
}

/// `<Vehicle Destruction>` — ship / vehicle blown up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleDestruction {
    pub timestamp: String,
    pub vehicle_class: String,
    pub vehicle_id: Option<String>,
    pub destroy_level: u8,
    pub caused_by: String,
    pub zone: Option<String>,
}

/// `<SHUDEvent_OnNotification>` — in-game banner notification queued
/// for the HUD. The text payload is human-readable and captures
/// jurisdiction crossings ("Entered Hurston Dynamics Jurisdiction"),
/// armistice-zone state changes, and other player-visible pop-ups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HudNotification {
    pub timestamp: String,
    pub text: String,
    pub notification_id: u64,
    pub mission_id: Option<String>,
}

/// `<RequestLocationInventory>` — fires when the player opens an
/// inventory at a location. The `Location[NAME]` field is the
/// strongest readable location signal we get — e.g. `Stanton2_Orison`,
/// `Stanton1_Lorville`. The placeholder `INVALID_LOCATION_ID` means
/// the player isn't yet bound to a known location (still loading or
/// in deep space).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationInventoryRequested {
    pub timestamp: String,
    pub player: String,
    pub location: String,
}

/// `<InvalidateAllTerrainCells>` — the engine drops a planet/moon's
/// terrain cell cache. Fires on planet load AND unload, so it's a
/// proximity signal rather than a strict "entered" event. The body
/// names the celestial body using its object-container key, e.g.
/// `OOC_Stanton_2b_Daymar` (Daymar) or `OOC_Stanton_1_Hurston`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanetTerrainLoad {
    pub timestamp: String,
    pub planet: String,
}

/// `<Player Selected Quantum Target - Local>` and
/// `<Player Requested Fuel to Quantum Target - Server Routing>` —
/// fires when the pilot picks a destination on the starmap. Single
/// line, no statefulness, captures the active vehicle and a readable
/// destination name (e.g. `OOC_Stanton_2_Crusader`, `Orison_LOC`).
///
/// `phase = Selected` means the player committed to the route;
/// `phase = FuelRequested` is the precursor where the engine is
/// computing whether the ship has enough fuel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuantumTargetSelected {
    pub timestamp: String,
    pub phase: QuantumTargetPhase,
    pub vehicle_class: String,
    pub vehicle_id: String,
    pub destination: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantumTargetPhase {
    FuelRequested,
    Selected,
}

/// `<CLandingArea::UnregisterFromExternalSystems>` — fires when the
/// engine retracts a landing area's external connections, which
/// happens when the ship is being stowed back into the player's
/// hangar / pad. Carries the landing area name (e.g.
/// `LandingArea_ShipElevator_HangarMediumTop` or
/// `[PROC]LandingArea_Pad_SmlB_{<guid>}`) plus the vehicle GEID, so
/// downstream code can reconstruct "this ship was last stowed here".
///
/// `zone_host_id` is `None` when the engine emits `[0]` — typically
/// for procedurally-generated outpost / reststop pads with no
/// persistent zone host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleStowed {
    pub timestamp: String,
    pub vehicle_id: String,
    pub landing_area: String,
    pub landing_area_id: String,
    pub zone_host_id: Option<String>,
}

/// `<AttachmentReceived>` — player gear / loadout. Fires when the
/// engine attaches an item to a body port, weapon-rail, or vehicle
/// module slot. Captures armor (`rsi_odyssey_undersuit_*`), weapons
/// (`klwe_pistol_energy_*`), multitool modules (`grin_multitool_*`),
/// and ship/vehicle equipment.
///
/// `elapsed_seconds` is the time since the entity was created — short
/// values (~0–30s) cluster around the player respawn / load-in burst,
/// longer values are real "I just equipped this" moments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentReceived {
    pub timestamp: String,
    pub player: String,
    pub item_class: String,
    pub item_id: String,
    pub status: String,
    pub port: String,
    pub elapsed_seconds: f64,
}

/// Synthetic event the client emits when it discovers a directory
/// under `<install>/<channel>/Crashes/`. Star Citizen drops a folder
/// per crash, named with an ISO-ish timestamp (e.g.
/// `2026-05-04-21-10-12`), containing a minidump plus one or more
/// `.log` files. We don't parse the dump body — the **fact of a
/// crash** is the signal worth surfacing on the timeline.
///
/// `crash_dir_name` is the stable identifier used for idempotency:
/// re-scanning the same Crashes/ folder must not produce duplicate
/// events even across client restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameCrash {
    /// Best-effort timestamp parsed from the crash dir name when it
    /// matches `YYYY-MM-DD-HH-MM-SS`; falls back to the dir mtime in
    /// RFC3339 form when parsing fails.
    pub timestamp: String,
    /// Channel the crash belongs to (LIVE/PTU/EPTU/...). Echoes the
    /// installed channel directory the crash dir lives under.
    pub channel: String,
    pub crash_dir_name: String,
    /// Filename of the largest `.log` file inside the crash dir, if
    /// any. The detail log usually carries an engine version banner
    /// and a stack trace; surfacing the filename lets a future wave
    /// pull and parse it without rewalking the filesystem.
    pub primary_log_name: Option<String>,
    /// On-disk size of the crash dir's contents in bytes (sum of
    /// every regular file inside). Lets the UI distinguish a
    /// trivial "engine couldn't init" crash from a fully-populated
    /// dump set.
    pub total_size_bytes: u64,
}

/// Synthetic event the client emits per recognised line in an RSI
/// Launcher log. The launcher writes Electron-style entries:
///
///   `[2026-05-06 12:34:56.789] [info] Some human-readable message`
///
/// We don't have a stable vocabulary the way `<Init>` / `<Join PU>`
/// give us in `Game.log`, but most launcher activity falls into a
/// handful of buckets — auth, install, patch, update, error — each
/// of which is signalled by predictable keywords in the message body.
/// `category` captures that classification so the timeline can group
/// by bucket without a per-row drilldown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherActivity {
    pub timestamp: String,
    /// One of `info`, `warn`, `error`, `debug`, ... — the bracketed
    /// level token from the launcher's log format. Lower-cased.
    pub level: String,
    /// The post-bracket body. Trimmed of leading/trailing whitespace.
    pub message: String,
    /// One of [`LauncherCategory`], serialised as a snake_case string.
    /// Derived from `(level, message)` keyword detection — see
    /// [`crate::parser::classify_launcher_message`].
    pub category: LauncherCategory,
}

/// Coarse bucket for launcher messages. Keep this list short — every
/// addition forces every consumer to handle the new variant. Anything
/// that doesn't match a specific bucket falls through to `Info` (the
/// useful default for human-readable status messages) or `Error` if
/// the level itself is `error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LauncherCategory {
    /// Login, logout, session refresh, credential prompts.
    Auth,
    /// Game install, verification, file integrity checks.
    Install,
    /// Patch download / apply / progress.
    Patch,
    /// Launcher self-update.
    Update,
    /// Anything at level=error or with explicit failure keywords.
    Error,
    /// Default catch-all for human status messages we don't bucket.
    Info,
}

// ---------------------------------------------------------------------
// Mission lifecycle
//
// Patterns reverse-engineered from external community captures (see
// `NOTICE`). Not present in this repo's session-only fixture, so the
// regexes ship as best-effort and may need tightening once we have a
// real mission capture under our own parser.
// ---------------------------------------------------------------------

/// Mission accepted / objective marker created. The engine emits
/// `<CLocalMissionPhaseMarker::CreateMarker>` when a player accepts a
/// contract, and a sibling `CreateMissionObjectiveMarker` call (no
/// shell brackets) when an individual objective spawns.
///
/// `mission_id` is the UUID the engine assigns; `marker_kind` records
/// whether this row came from the phase marker or an objective marker
/// so consumers can suppress duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionStart {
    pub timestamp: String,
    pub mission_id: String,
    pub marker_kind: MissionMarkerKind,
    pub mission_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionMarkerKind {
    /// `<CLocalMissionPhaseMarker::CreateMarker>` — top-level mission
    /// acceptance.
    Phase,
    /// `CreateMissionObjectiveMarker` — sub-objective beacon.
    Objective,
}

/// `<EndMission>` — mission completed, failed, or abandoned. The
/// engine doesn't reliably emit a status field; consumers can pair
/// this with the most recent `MissionStart` to compute duration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionEnd {
    pub timestamp: String,
    pub mission_id: Option<String>,
    pub outcome: Option<String>,
}

// ---------------------------------------------------------------------
// Shop / commodity transactions
//
// These come in pairs:
//   1. `Send*Request` — client-side optimistic submit (treat as pending)
//   2. `*FlowResponse` — server confirmation (treat as confirmed)
//
// We surface both halves and let downstream code (gamelog state
// machine, tray UI) reconcile. A pending request without a matching
// response within ~30s should be considered failed.
// ---------------------------------------------------------------------

/// `SendShopBuyRequest` / `SendStandardItemBuyRequest` — player
/// clicked Buy in a kiosk. Optimistic; not yet confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopBuyRequest {
    pub timestamp: String,
    pub shop_id: Option<String>,
    pub item_class: Option<String>,
    pub quantity: Option<u32>,
    pub raw: String,
}

/// `ShopFlowResponse` — server-side confirmation (or rejection) for
/// a previously-sent shop request. `success` is best-effort: derived
/// from a `Result[Success]` / `Result[OK]` token in the body when
/// present, otherwise `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopFlowResponse {
    pub timestamp: String,
    pub shop_id: Option<String>,
    pub success: Option<bool>,
    pub raw: String,
}

/// `SendCommodityBuyRequest` — commodity terminal purchase (e.g.
/// fuel, refined ore, agricium). Pending until the corresponding
/// flow response lands.
///
/// No `Eq` derive: `quantity` is `Option<f64>`, and `f64` only
/// implements `PartialEq`. The wider `GameEvent` enum already lacks
/// `Eq` for the same reason, so this is consistent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommodityBuyRequest {
    pub timestamp: String,
    pub commodity: Option<String>,
    pub quantity: Option<f64>,
    pub raw: String,
}

/// `SendCommoditySellRequest` — commodity terminal sale. Same
/// `Eq`-vs-`f64` constraint as [`CommodityBuyRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommoditySellRequest {
    pub timestamp: String,
    pub commodity: Option<String>,
    pub quantity: Option<f64>,
    pub raw: String,
}

// ---------------------------------------------------------------------
// Session boundary
// ---------------------------------------------------------------------

/// `<SystemQuit>` / `CCIGBroker::FastShutdown` — clean session
/// terminator. Pairs with [`ProcessInit`] to bound a play session.
/// Emitted as a single event regardless of which token the engine
/// printed; `kind` records which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnd {
    pub timestamp: String,
    pub kind: SessionEndKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndKind {
    /// `<SystemQuit>` shell event.
    SystemQuit,
    /// `CCIGBroker::FastShutdown` function-style entry.
    FastShutdown,
}

// ---------------------------------------------------------------------
// Dynamic parser-definition support (see docs/PARSER_DEFINITION_UPDATES.md)
//
// `RemoteMatch` is the catch-all variant the parser emits when a
// remote rule (fetched from `GET /v1/parser-definitions`) matches a
// log line that the built-in classifier didn't recognise. The
// `event_name` carries the rule's declared name so timeline
// consumers can render it; `fields` is the rule's named-capture
// extraction. `rule_id` is the manifest's stable id, surfaced so a
// buggy rule can be retracted without rebuilding the client.
// ---------------------------------------------------------------------

/// Event emitted by a runtime-loaded parser rule. Persisted with the
/// usual ingest pipeline so users see one consistent event surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteMatch {
    pub timestamp: String,
    pub rule_id: String,
    pub event_name: String,
    pub fields: std::collections::BTreeMap<String, String>,
}

// ---------------------------------------------------------------------
// Burst-collapse aggregate (see crate::templates::BurstRule)
//
// Emitted by the tray's gamelog ingest when a `BurstRule` fires on a
// run of N+ semantically-equivalent log lines (e.g. the 20+
// `<AttachmentReceived>` shower fired by a player respawn, or the
// `<StatObjLoad>` blast during planet entry). The constituent member
// events are NOT uploaded — one summary stands in for the whole
// group on the server timeline, while the local tray cache retains
// the raw members for drill-in.
// ---------------------------------------------------------------------

/// Aggregate event for a collapsed burst.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurstSummary {
    /// Anchor line's timestamp — used as the ordering key on the
    /// timeline. ISO-8601 UTC, same shape as every other event.
    pub timestamp: String,
    /// `BurstRule.id` that fired (e.g. `"loadout_restore_burst"`).
    /// Drives downstream rendering and aggregation.
    pub rule_id: String,
    /// Total members in the burst (anchor + follow-ups). Always
    /// `>= rule.min_burst_size`.
    pub size: u32,
    /// Last member's timestamp. Same as `timestamp` for atomic bursts
    /// (loadout-restore is one millisecond); later for time-spread
    /// runs. Lets the timeline show "burst of 20 attachments over 0.5s".
    pub end_timestamp: String,
    /// Truncated copy of the anchor line's body (capped at 200 chars
    /// in the producer). Lets a generic timeline render
    /// "burst started with: body_01_noMagicPocket..." without storing
    /// every member. `None` if the producer didn't sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_body_sample: Option<String>,
}
