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
use std::collections::{BTreeMap, HashSet};

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

// ─── Interest score ─────────────────────────────────────────────────

/// Borrowed context for [`interest_score`]. Caller owns the HashSets;
/// this struct keeps the function signature short and lets us add new
/// inputs later without breaking call sites that build from a
/// long-lived runtime cache.
pub struct InterestContext<'a> {
    /// Shell tags the parser has built-in or remote rules for.
    pub known_shell_tags: &'a HashSet<String>,
    /// Subset of known shell tags that have at least one remote rule
    /// targeting them. Tags in `known_shell_tags` but not here are
    /// tags the parser knows about but doesn't classify (yet).
    pub known_rule_tags: &'a HashSet<String>,
    pub session_occurrence_count: u32,
    pub multi_session: bool,
    pub already_remote_matched: bool,
}

/// Heuristic 0..=100 for how likely a line carries useful event
/// signal. The UI surfaces lines above a configurable threshold
/// (default 50). Tuning knobs:
///
/// * Unknown shell tag → +40 (strongest single signal).
/// * Known shell tag with no remote rule → +30 (gap in coverage).
/// * GEID-shaped digit cluster in body → +15.
/// * Body keywords (`OOC_`, `body_`, `_class`) → +10.
/// * Repeated this session → +10, multi-session → +20 (sustained, not
///   one-off noise).
/// * Extremely short or long → −30 (not an event we can usefully
///   parse).
/// * `already_remote_matched` short-circuits to 0 — a matched line is
///   not unknown.
pub fn interest_score(line: &str, shell_tag: Option<&str>, ctx: &InterestContext) -> u8 {
    if ctx.already_remote_matched {
        return 0;
    }
    let mut score: i32 = 0;
    if let Some(tag) = shell_tag {
        if !ctx.known_shell_tags.contains(tag) {
            score += 40;
        } else if !ctx.known_rule_tags.contains(tag) {
            score += 30;
        }
    }
    if line.contains('[') && line.chars().filter(|c| c.is_ascii_digit()).count() >= 5 {
        score += 15;
    }
    if line.contains("OOC_") || line.contains("body_") || line.contains("_class") {
        score += 10;
    }
    if ctx.session_occurrence_count >= 3 {
        score += 10;
    }
    if ctx.multi_session {
        score += 20;
    }
    let len = line.len();
    if !(20..=2000).contains(&len) {
        score -= 30;
    }
    score.clamp(0, 100) as u8
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

    // ─── Interest score ──────────────────────────────────────────────

    fn known_tags(tags: &[&str]) -> HashSet<String> {
        tags.iter().map(|s| (*s).to_string()).collect()
    }

    fn ctx_with<'a>(
        known_shell: &'a HashSet<String>,
        known_rule: &'a HashSet<String>,
    ) -> InterestContext<'a> {
        InterestContext {
            known_shell_tags: known_shell,
            known_rule_tags: known_rule,
            session_occurrence_count: 0,
            multi_session: false,
            already_remote_matched: false,
        }
    }

    #[test]
    fn unknown_shell_tag_surfaces_above_threshold() {
        let known_shell = HashSet::new();
        let known_rule = HashSet::new();
        let ctx = ctx_with(&known_shell, &known_rule);
        let line = "<2026-05-17T14:02:30Z> [Notice] <NewMystery> body with id [54324]";
        let score = interest_score(line, Some("NewMystery"), &ctx);
        // +40 unknown shell, +15 GEID-like cluster = 55, comfortably > 50.
        assert!(score >= 50, "expected surfacing score, got {score}");
    }

    #[test]
    fn known_tag_without_rule_scores_below_unknown() {
        let known_shell = known_tags(&["PartiallyKnown"]);
        let known_rule = HashSet::new();
        let ctx = ctx_with(&known_shell, &known_rule);
        // No '[' in the line so the GEID bonus doesn't fire — isolate the
        // +30 gap-in-coverage contribution.
        let line = "<2026-05-17T14:02:30Z> Notice PartiallyKnown short text";
        let score = interest_score(line, Some("PartiallyKnown"), &ctx);
        assert_eq!(score, 30);
    }

    #[test]
    fn already_remote_matched_short_circuits_to_zero() {
        let known_shell = HashSet::new();
        let known_rule = HashSet::new();
        let mut ctx = ctx_with(&known_shell, &known_rule);
        ctx.already_remote_matched = true;
        let line = "<2026-05-17T14:02:30Z> [Notice] <NewMystery> body with id [54324]";
        assert_eq!(interest_score(line, Some("NewMystery"), &ctx), 0);
    }

    #[test]
    fn fully_classified_tag_scores_zero() {
        let known_shell = known_tags(&["FullyKnown"]);
        let known_rule = known_tags(&["FullyKnown"]);
        let ctx = ctx_with(&known_shell, &known_rule);
        // No '[' in the line, no GEID bonus — isolate the fully-classified case.
        let line = "<2026-05-17T14:02:30Z> Notice FullyKnown some short body";
        assert_eq!(interest_score(line, Some("FullyKnown"), &ctx), 0);
    }

    #[test]
    fn repeated_lines_score_higher() {
        let known_shell = HashSet::new();
        let known_rule = HashSet::new();
        let mut ctx = ctx_with(&known_shell, &known_rule);
        ctx.session_occurrence_count = 5;
        ctx.multi_session = true;
        let line = "<2026-05-17T14:02:30Z> [Notice] <NewMystery> body with id [54324]";
        let score = interest_score(line, Some("NewMystery"), &ctx);
        // +40 unknown +15 GEID +10 repeats +20 multi-session = 85.
        assert_eq!(score, 85);
    }

    #[test]
    fn keyword_bonus_for_body_class_etc() {
        let known_shell = HashSet::new();
        let known_rule = HashSet::new();
        let ctx = ctx_with(&known_shell, &known_rule);
        let line =
            "<2026-05-17T14:02:30Z> [Notice] <NewMystery> killed body_01_noMagicPocket id [54324]";
        let score = interest_score(line, Some("NewMystery"), &ctx);
        // +40 unknown +15 GEID +10 keyword.
        assert_eq!(score, 65);
    }

    #[test]
    fn very_short_or_long_lines_penalised() {
        let known_shell = HashSet::new();
        let known_rule = HashSet::new();
        let ctx = ctx_with(&known_shell, &known_rule);
        let short = "<X> <Foo>";
        // Unknown tag (+40) but len < 20 (−30) = 10, well below threshold.
        let score = interest_score(short, Some("Foo"), &ctx);
        assert!(score < 50);
    }
}
