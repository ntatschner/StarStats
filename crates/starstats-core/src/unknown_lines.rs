//! Capture and characterise lines that didn't match any built-in
//! classifier OR a remote parser rule. These flow to a local review
//! queue in the tray; the user opts in to submitting promising ones
//! back to the rule-author moderation queue on the server.
//!
//! Three concerns split across this module:
//!
//! 1. **Shape normalisation** — collapse a raw line down to a template
//!    with identifiers replaced by placeholder tokens. Same template,
//!    same `shape_hash`, so the tray can dedupe spam.
//! 2. **Interest score** — heuristic 0..=100 ranking how likely the
//!    line carries useful event signal. (Task 30.)
//! 3. **PII detection** — pre-flag handles, shard IDs, GEIDs, IPs so
//!    the user reviews redaction before submission. (Task 31.)
//!
//! Submission itself is *not* in this module — that lives in the tray
//! crate (Phase 4.B) and the server endpoint (Phase 4.C). This module
//! is pure types + functions, callable from any consumer.

use crate::wire::LogSource;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One pre-existing-shape-normalised candidate.
///
/// Stored locally in the tray's SQLite cache (one row per `shape_hash`,
/// with `occurrence_count` tracking how many raw lines collapsed to
/// the same shape this session). Never auto-uploaded — only the user
/// can submit, and only after redaction review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownLine {
    pub id: String,
    pub raw_line: String,
    pub timestamp: Option<String>,
    pub shell_tag: Option<String>,
    pub partial_structured: BTreeMap<String, String>,
    /// Last 5 lines before this one in source order.
    pub context_before: Vec<String>,
    /// Up to 5 lines after — filled lazily as they arrive.
    pub context_after: Vec<String>,
    pub game_build: Option<String>,
    pub channel: LogSource,
    pub interest_score: u8,
    pub shape_hash: String,
    pub occurrence_count: u32,
    pub first_seen: String,
    pub last_seen: String,
    pub detected_pii: Vec<PiiToken>,
    pub dismissed: bool,
}

/// One auto-detected sensitive token in a raw line. Filled in by
/// [`detect_pii`] (Task 31); declared here so `UnknownLine` carries
/// the field on the wire from day one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PiiToken {
    pub kind: PiiKind,
    pub start: usize,
    pub end: usize,
    pub suggested_redaction: String,
    pub default_redact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiKind {
    OwnHandle,
    FriendHandle,
    ShardId,
    Geid,
    IpPort,
}

// ─── Shape normalisation ────────────────────────────────────────────

static TS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?").expect("TS_RE compiles")
});
static UUID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .expect("UUID_RE compiles")
});
static GEID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\d{5,}\]").expect("GEID_RE compiles"));
static IPPORT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(?::\d{1,5})?\b").expect("IPPORT_RE compiles")
});
static QUOTED_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#""[^"]+""#).expect("QUOTED_RE compiles"));

/// Collapse a raw log line to its shape: identifiers and timestamps
/// become tokens like `<TS>`, `<GEID>`, etc. Same template → same shape.
pub fn shape_of(line: &str) -> String {
    let s = TS_RE.replace_all(line, "<TS>");
    let s = UUID_RE.replace_all(&s, "<UUID>");
    let s = GEID_RE.replace_all(&s, "[<GEID>]");
    let s = IPPORT_RE.replace_all(&s, "<IPPORT>");
    let s = QUOTED_RE.replace_all(&s, "\"<STR>\"");
    s.into_owned()
}

/// Stable hash of a shape, fitting in a SQLite TEXT column. The `sh_`
/// prefix makes it self-describing in row dumps; the 16 hex chars come
/// from `DefaultHasher` which is good enough for dedupe (not crypto).
pub fn shape_hash(line: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    shape_of(line).hash(&mut h);
    format!("sh_{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn shape_stable_across_value_changes() {
        let a = shape_of("<2026-01-01T00:00:00Z> [X] <Foo> id [12345]");
        let b = shape_of("<2026-05-17T14:02:31Z> [X] <Foo> id [54324]");
        assert_eq!(a, b);
    }

    #[test]
    fn shape_hash_stable() {
        let h1 = shape_hash("<2026-01-01T00:00:00Z> [X] <Foo>");
        let h2 = shape_hash("<2026-12-31T23:59:59Z> [X] <Foo>");
        assert_eq!(h1, h2);
    }

    #[test]
    fn shape_collapses_ip_port_and_quoted_strings() {
        let a = shape_of(r#"<2026-01-01T00:00:00Z> connect 1.2.3.4:64300 name="alice""#);
        let b = shape_of(r#"<2026-12-31T23:59:59Z> connect 9.8.7.6:65000 name="bob""#);
        assert_eq!(a, b);
        assert!(a.contains("<IPPORT>"));
        assert!(a.contains("\"<STR>\""));
    }
}
