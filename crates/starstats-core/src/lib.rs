//! `starstats-core` — wire types, log parser, validators shared by the
//! tray client and the API server.
//!
//! Design rule: this crate must compile on every platform we ship to
//! (Win, Linux, macOS) and depend on **no** runtime / framework crates.
//! It's pure types + functions. Anything async or I/O lives in the
//! consuming crates.

pub mod events;
pub mod inference;
pub mod metadata;
pub mod parser;
pub mod parser_defs;
pub mod templates;
pub mod transactions;
pub mod unknown_lines;
pub mod validators;
pub mod wire;

pub use events::{
    ActorDeath, AttachmentReceived, BurstSummary, ChangeServer, CommodityBuyRequest,
    CommoditySellRequest, GameCrash, GameEvent, HudNotification, JoinPu, LauncherActivity,
    LauncherCategory, LegacyLogin, LocationChanged, LocationInventoryRequested, MissionEnd,
    MissionMarkerKind, MissionStart, PlanetTerrainLoad, PlayerDeath, PlayerIncapacitated,
    ProcessInit, QuantumTargetPhase, QuantumTargetSelected, RemoteMatch, ResolveSpawn,
    SeedSolarSystem, ServerPhase, SessionEnd, SessionEndKind, ShopBuyRequest, ShopFlowResponse,
    ShopRequestTimedOut, VehicleDestruction, VehicleStowed,
};
pub use inference::{infer, InferenceConfig, InferredEvent};
pub use metadata::{
    event_type_key, group_key_for, primary_entity_for, provenance_for_inferred_field, stamp,
    EntityKind, EntityRef, EventMetadata, EventSource, FieldProvenance,
};
pub use parser::{
    classify, classify_launcher_message, classify_with_metadata, parse_launcher_line,
    structural_parse, LauncherLogLine, LogLine, ParseStats,
};
pub use parser_defs::{
    apply_remote_rules, compile_rules, CompiledRemoteRule, Manifest, RemoteRule, RuleMatchKind,
};
pub use transactions::{pair_transactions, Transaction, TransactionKind, TransactionStatus};
pub use unknown_lines::{shape_hash, shape_of, PiiKind, PiiToken, UnknownLine};
pub use validators::{validate_event, validate_metadata, ValidationError};
pub use wire::{EventEnvelope, IngestBatch};
