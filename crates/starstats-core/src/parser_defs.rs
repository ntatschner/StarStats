//! Runtime-loaded parser rules — the data + apply layer for the
//! dynamic-parser-definition feature.
//!
//! Wire format (`RemoteRule`) is what the server's
//! `GET /v1/parser-definitions` endpoint returns. `CompiledRemoteRule`
//! is what the client holds at runtime once the regex has been
//! pre-compiled. `apply_remote_rules` runs after the built-in
//! `classify` returns `None` — it never overrides a built-in match.
//!
//! Architectural rule: this crate stays I/O-free. Fetching, caching,
//! and signature verification live in the consuming crates
//! (`starstats-client` for the fetcher, `starstats-server` for the
//! manifest hosting). All this module does is parse + match.

use crate::events::{GameEvent, RemoteMatch};
use crate::parser::LogLine;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One rule on the wire. Mirrors the JSON shape documented in
/// `docs/PARSER_DEFINITION_UPDATES.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRule {
    /// Stable id assigned by the manifest publisher. Used to retract
    /// a bad rule without rebuilding clients (server publishes a
    /// fresh manifest with the rule absent or with a `disabled`
    /// flag — for v1 we just rely on absence).
    pub id: String,
    /// Either the `<EventName>` token to match against `LogLine.event_name`
    /// (when the line has a shell), OR a body-keyword to match against
    /// `LogLine.body` for function-call-style entries. The rule's
    /// `match_kind` disambiguates.
    pub event_name: String,
    /// `event_name` matches the `<EventName>` shell.
    /// `body_keyword` matches if `body.contains(event_name)`.
    #[serde(default = "default_match_kind")]
    pub match_kind: RuleMatchKind,
    /// Body regex with optional named captures. Captures listed in
    /// `fields` get extracted into `RemoteMatch.fields`. Anything else
    /// is ignored — extra captures don't error, missing captures don't
    /// fail the match (their fields just don't appear in the output).
    pub body_regex: String,
    /// Names of regex captures to surface as fields. Order is
    /// preserved in the BTreeMap keys for deterministic JSON output.
    #[serde(default)]
    pub fields: Vec<String>,
}

fn default_match_kind() -> RuleMatchKind {
    RuleMatchKind::EventName
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchKind {
    /// Match `LogLine.event_name == rule.event_name`.
    EventName,
    /// Match `LogLine.body.contains(rule.event_name)`.
    BodyKeyword,
}

/// The full manifest shape returned by the server endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub schema_version: u32,
    pub issued_at: String,
    pub rules: Vec<RemoteRule>,
    /// Optional ed25519 signature over the canonicalised `rules`
    /// array. v1 ships unverified — clients trust TLS to the server.
    /// Verification is a follow-up.
    #[serde(default)]
    pub signature: Option<String>,
}

impl Manifest {
    pub fn empty() -> Self {
        Self {
            version: 0,
            schema_version: 1,
            issued_at: String::new(),
            rules: Vec::new(),
            signature: None,
        }
    }
}

/// Runtime-ready rule with a pre-compiled regex. Constructed via
/// [`compile_rules`]; rules whose regex fails to compile are silently
/// dropped (the caller logs the failure during fetch).
#[derive(Debug, Clone)]
pub struct CompiledRemoteRule {
    pub id: String,
    pub event_name: String,
    pub match_kind: RuleMatchKind,
    pub regex: Regex,
    pub fields: Vec<String>,
}

/// Compile a slice of wire-format rules. Returns the compiled subset
/// + a Vec of `(rule_id, error_message)` pairs for any rules whose
/// regex failed to compile. The caller logs the errors; bad rules
/// are not fatal.
pub fn compile_rules(rules: &[RemoteRule]) -> (Vec<CompiledRemoteRule>, Vec<(String, String)>) {
    let mut ok = Vec::with_capacity(rules.len());
    let mut bad = Vec::new();
    for r in rules {
        match Regex::new(&r.body_regex) {
            Ok(rx) => ok.push(CompiledRemoteRule {
                id: r.id.clone(),
                event_name: r.event_name.clone(),
                match_kind: r.match_kind,
                regex: rx,
                fields: r.fields.clone(),
            }),
            Err(e) => bad.push((r.id.clone(), e.to_string())),
        }
    }
    (ok, bad)
}

/// Try the cached remote rules against a log line. Returns the first
/// match or `None`. Order in `rules` matters — rule authors who care
/// about specificity should put narrower rules first.
///
/// This function does not run if the built-in `classify` already
/// produced a `Some` — see the gamelog ingest path. That guarantees
/// remote rules can only *add* recognition, never override built-ins.
pub fn apply_remote_rules(line: &LogLine<'_>, rules: &[CompiledRemoteRule]) -> Option<GameEvent> {
    for rule in rules {
        let matches_anchor = match rule.match_kind {
            RuleMatchKind::EventName => line.event_name == Some(rule.event_name.as_str()),
            RuleMatchKind::BodyKeyword => line.body.contains(rule.event_name.as_str()),
        };
        if !matches_anchor {
            continue;
        }

        let caps = rule.regex.captures(line.body)?;
        let mut fields = BTreeMap::new();
        for f in &rule.fields {
            if let Some(m) = caps.name(f) {
                fields.insert(f.clone(), m.as_str().to_string());
            }
        }
        return Some(GameEvent::RemoteMatch(RemoteMatch {
            timestamp: line.timestamp.to_string(),
            rule_id: rule.id.clone(),
            event_name: rule.event_name.clone(),
            fields,
        }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::structural_parse;

    fn rule(id: &str, event: &str, kind: RuleMatchKind, rx: &str, fields: &[&str]) -> RemoteRule {
        RemoteRule {
            id: id.to_string(),
            event_name: event.to_string(),
            match_kind: kind,
            body_regex: rx.to_string(),
            fields: fields.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn matches_event_name_anchor() {
        let rules = vec![rule(
            "r1",
            "PlayerDance",
            RuleMatchKind::EventName,
            r"emote=(?P<emote>\w+)",
            &["emote"],
        )];
        let (compiled, bad) = compile_rules(&rules);
        assert!(bad.is_empty());
        let line = "<2026-05-07T15:00:00.000Z> [Notice] <PlayerDance> emote=salute [Team_X]";
        let parsed = structural_parse(line).unwrap();
        let ev = apply_remote_rules(&parsed, &compiled).unwrap();
        match ev {
            GameEvent::RemoteMatch(m) => {
                assert_eq!(m.event_name, "PlayerDance");
                assert_eq!(m.rule_id, "r1");
                assert_eq!(m.fields.get("emote").map(|s| s.as_str()), Some("salute"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn matches_body_keyword_anchor() {
        let rules = vec![rule(
            "r2",
            "SendCustomThing",
            RuleMatchKind::BodyKeyword,
            r"shopId=(?P<shop>\w+)",
            &["shop"],
        )];
        let (compiled, _) = compile_rules(&rules);
        // No <EventName> shell — function-call-style line.
        let line = "<2026-05-07T15:00:00.000Z> [Notice] SendCustomThing(shopId=area18, qty=1)";
        let parsed = structural_parse(line).unwrap();
        let ev = apply_remote_rules(&parsed, &compiled).unwrap();
        match ev {
            GameEvent::RemoteMatch(m) => {
                assert_eq!(m.fields.get("shop").map(|s| s.as_str()), Some("area18"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn returns_none_when_no_rule_matches() {
        let rules = vec![rule(
            "r1",
            "SomethingElse",
            RuleMatchKind::EventName,
            r"x=(?P<x>\d+)",
            &["x"],
        )];
        let (compiled, _) = compile_rules(&rules);
        let line = "<2026-05-07T15:00:00.000Z> [Notice] <PlayerDance> emote=salute";
        let parsed = structural_parse(line).unwrap();
        assert!(apply_remote_rules(&parsed, &compiled).is_none());
    }

    #[test]
    fn bad_regex_lands_in_error_list_not_compiled() {
        let rules = vec![rule(
            "r1",
            "X",
            RuleMatchKind::EventName,
            "[unclosed",
            &["x"],
        )];
        let (ok, bad) = compile_rules(&rules);
        assert!(ok.is_empty());
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].0, "r1");
    }

    #[test]
    fn missing_capture_field_is_silently_omitted() {
        // Regex matches but doesn't capture `expected_field` — the
        // output map should just lack that key, not error.
        let rules = vec![rule(
            "r1",
            "X",
            RuleMatchKind::EventName,
            r"present=(?P<present>\w+)",
            &["present", "expected_field"],
        )];
        let (compiled, _) = compile_rules(&rules);
        let line = "<2026-05-07T15:00:00.000Z> [Notice] <X> present=hi";
        let parsed = structural_parse(line).unwrap();
        let ev = apply_remote_rules(&parsed, &compiled).unwrap();
        match ev {
            GameEvent::RemoteMatch(m) => {
                assert!(m.fields.contains_key("present"));
                assert!(!m.fields.contains_key("expected_field"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
