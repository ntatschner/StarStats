//! `starstats-core` — wire types, log parser, validators shared by the
//! tray client and the API server.
//!
//! Design rule: this crate must compile on every platform we ship to
//! (Win, Linux, macOS) and depend on **no** runtime / framework crates.
//! It's pure types + functions. Anything async or I/O lives in the
//! consuming crates.

pub mod events;
pub mod parser;
pub mod parser_defs;
pub mod templates;
pub mod transactions;
pub mod validators;
pub mod wire;

pub use events::{
    ActorDeath, AttachmentReceived, ChangeServer, CommodityBuyRequest, CommoditySellRequest,
    GameCrash, GameEvent, HudNotification, JoinPu, LauncherActivity, LauncherCategory, LegacyLogin,
    LocationInventoryRequested, MissionEnd, MissionMarkerKind, MissionStart, PlanetTerrainLoad,
    PlayerDeath, PlayerIncapacitated, ProcessInit, QuantumTargetPhase, QuantumTargetSelected,
    RemoteMatch, ResolveSpawn, SeedSolarSystem, ServerPhase, SessionEnd, SessionEndKind,
    ShopBuyRequest, ShopFlowResponse, VehicleDestruction, VehicleStowed,
};
pub use parser::{
    classify, classify_launcher_message, parse_launcher_line, structural_parse, LauncherLogLine,
    LogLine, ParseStats,
};
pub use parser_defs::{
    apply_remote_rules, compile_rules, CompiledRemoteRule, Manifest, RemoteRule, RuleMatchKind,
};
pub use transactions::{pair_transactions, Transaction, TransactionKind, TransactionStatus};
pub use validators::{validate_event, ValidationError};
pub use wire::{EventEnvelope, IngestBatch};
