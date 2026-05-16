# Tray Setup & Health Surface — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a unified Health surface that aggregates every actionable setup/lifetime problem into one top-of-Status card, plus inline validation probes for API URL and RSI cookie.

**Architecture:** New Rust module `health.rs` (pure derivation over a `HealthInputs` snapshot) + `probes.rs` (opt-in HTTPS probes), 4 new Tauri commands wired through `commands.rs`. New React components `HealthCard` + `InlineCheck` driven by hooks `useHealth` + `useFieldFocus`, with string mapping in `useHealthStrings.ts` and error normalisation in `friendlyError.ts`. Vitest is being added to `apps/tray-ui/` as part of this work since no frontend test runner exists today.

**Tech Stack:** Rust (axum-free; reqwest, serde, sysinfo, tokio), TypeScript + React 19, Tauri 2, Vitest + @testing-library/react.

**Branch:** `feat/tray-health-surface` (created from `main`). All commits land on this branch; final push is to the branch, not `main`.

**Spec:** `docs/superpowers/specs/2026-05-16-tray-setup-health-design.md`

---

## File Structure

**Rust — `crates/starstats-client/src/`:**
- `health.rs` *(new)* — `HealthItem`/`HealthId`/`HealthParams`/etc. types, `HealthInputs` snapshot, `current_health()` pure derivation, fingerprint helper, dismissal filter.
- `probes.rs` *(new)* — `ApiUrlCheck`, `CookieCheck` types; `check_api_url()`, `check_rsi_cookie()` async functions.
- `commands.rs` *(modify)* — add 4 thin Tauri command wrappers: `get_health`, `dismiss_health`, `check_api_url`, `check_rsi_cookie`.
- `config.rs` *(modify)* — add `dismissed_health: Vec<DismissedHealth>` field on `Config` (serde-defaulted).
- `state.rs` *(modify)* — add `update_available: Arc<parking_lot::Mutex<Option<UpdateInfo>>>` field on `AppState`.
- `main.rs` *(modify)* — register the 4 new commands in `invoke_handler!`; populate `update_available` after the auto-update check.

> All required dependencies (`sysinfo`, `chrono`, `serde`, `serde_json`, `anyhow`, `reqwest`, `tokio`, `parking_lot`) are already declared in `crates/starstats-client/Cargo.toml`. No Cargo.toml changes are required.

**Frontend — `apps/tray-ui/`:**
- `package.json` *(modify)* — add Vitest + jsdom + testing-library deps, add `test` and `test:run` scripts.
- `vitest.config.ts` *(new)* — Vitest config (jsdom env, setup file).
- `src/test/setup.ts` *(new)* — test setup with Tauri `invoke()` mock.
- `src/api.ts` *(modify)* — add TypeScript types mirroring Rust + 4 new `api.*` methods.
- `src/lib/friendlyError.ts` *(new)* — pure mapper `unknown → FriendlyError`.
- `src/lib/friendlyError.test.ts` *(new)* — Vitest unit tests.
- `src/lib/useHealthStrings.ts` *(new)* — `HealthParams` → `{summary, detail?}` mapper.
- `src/lib/useHealthStrings.test.ts` *(new)* — exhaustive variant tests.
- `src/hooks/useFieldFocus.tsx` *(new)* — context + ref registry for cross-pane field focus.
- `src/hooks/useHealth.ts` *(new)* — polls `getHealth()` at 15s cadence.
- `src/components/HealthCard.tsx` *(new)* — renders the health item list (or `null` when empty).
- `src/components/HealthCard.test.tsx` *(new)* — component tests with fixture lists.
- `src/components/InlineCheck.tsx` *(new)* — generic Test button + state UI.
- `src/components/InlineCheck.test.tsx` *(new)* — component tests for idle/running/ok/err states.
- `src/App.tsx` *(modify)* — wrap `<main>` with `<FieldFocusProvider>`.
- `src/components/StatusPane.tsx` *(modify)* — render `<HealthCard>` at top; remove the two `<Banner>` blocks for `auth_lost` and `email_unverified`.
- `src/components/SettingsPane.tsx` *(modify)* — register field refs via `useFieldFocus`, add `<InlineCheck>` next to API URL input and RSI cookie input, swap `String(err)` for `friendlyError()` in catch blocks.

**Tests — `crates/starstats-client/src/`:**
- All Rust tests are co-located in the same modules under `#[cfg(test)] mod tests { ... }`.

---

## Pre-flight

- [ ] **Step 0.1: Create the working branch**

```bash
git checkout -b feat/tray-health-surface
git status
```

Expected: branch created, status shows `?? docs/superpowers/` (the spec + this plan, untracked).

- [ ] **Step 0.2: Stage and commit the spec + this plan as the first commit on the branch**

```bash
git add docs/superpowers/specs/2026-05-16-tray-setup-health-design.md \
        docs/superpowers/plans/2026-05-16-tray-setup-health.md
git commit -m "docs: spec and implementation plan for tray health surface"
```

Expected: one commit on `feat/tray-health-surface` containing only the two markdown files.

---

## Phase 1 — Rust foundation

### Task 1: Skeleton `health.rs` module with types

**Files:**
- Create: `crates/starstats-client/src/health.rs`
- Modify: `crates/starstats-client/src/main.rs:<top of file, mod declarations block>`

- [ ] **Step 1.1: Add `mod health;` to `main.rs` declarations**

Open `crates/starstats-client/src/main.rs`, find the existing `mod foo;` block near the top (alongside `mod commands;`, `mod config;`, `mod gamelog;`, etc.). Add:

```rust
mod health;
```

Keep the block alphabetised.

- [ ] **Step 1.2: Create `health.rs` with the data types only (no logic yet)**

```rust
//! Aggregated health surface for the tray UI.
//!
//! `current_health()` is a *pure* function over a `HealthInputs`
//! snapshot — it does no I/O. The caller (`commands::get_health`) is
//! responsible for assembling the snapshot from `AppState`, `Config`,
//! the secret store, and `sysinfo`. Keeping derivation pure lets us
//! exhaustively unit-test every check from in-memory fixtures.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HealthId {
    GamelogMissing,
    ApiUrlMissing,
    PairMissing,
    AuthLost,
    CookieMissing,
    SyncFailing,
    HangarSkip,
    EmailUnverified,
    GameLogStale,
    UpdateAvailable,
    DiskFreeLow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SettingsField {
    GamelogPath,
    ApiUrl,
    PairingCode,
    RsiCookie,
    Updates,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthAction {
    GoToSettings { field: SettingsField },
    RetrySync,
    RefreshHangar,
    OpenUrl { url: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "id", rename_all = "snake_case")]
pub enum HealthParams {
    GamelogMissing,
    ApiUrlMissing,
    PairMissing,
    AuthLost,
    CookieMissing,
    SyncFailing { last_error: String, attempts_since_success: u32 },
    HangarSkip { reason: String, since: String },
    EmailUnverified,
    GameLogStale { last_event_at: String },
    UpdateAvailable { version: String },
    DiskFreeLow { free_bytes: u64 },
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthItem {
    pub id: HealthId,
    pub severity: Severity,
    pub params: HealthParams,
    pub action: Option<HealthAction>,
    pub dismissible: bool,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DismissedHealth {
    pub id: HealthId,
    pub fingerprint: String,
    pub dismissed_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_serialises_snake_case() {
        let s = serde_json::to_string(&Severity::Error).unwrap();
        assert_eq!(s, "\"error\"");
    }

    #[test]
    fn health_action_uses_kind_tag() {
        let action = HealthAction::GoToSettings {
            field: SettingsField::ApiUrl,
        };
        let s = serde_json::to_string(&action).unwrap();
        assert!(s.contains("\"kind\":\"go_to_settings\""));
        assert!(s.contains("\"field\":\"api_url\""));
    }
}
```

- [ ] **Step 1.3: Run cargo build**

```bash
cargo build -p starstats-client
```

Expected: clean build (`Finished dev` line). No warnings about unused `Deserialize` / `Eq` derives are tolerated for the types above; if any appear, leave them — they'll be exercised later.

- [ ] **Step 1.4: Run the two skeleton tests**

```bash
cargo test -p starstats-client --lib health::
```

Expected: 2 passed, 0 failed.

- [ ] **Step 1.5: Commit**

```bash
git add crates/starstats-client/src/health.rs crates/starstats-client/src/main.rs
git commit -m "feat(health): add HealthItem types and serde shape"
```

---

### Task 2: `HealthInputs` snapshot + `current_health()` skeleton

**Files:**
- Modify: `crates/starstats-client/src/health.rs`

- [ ] **Step 2.1: Add `HealthInputs` and an empty `current_health()` at the end of `health.rs` (before `#[cfg(test)]`)**

```rust
/// Read-only snapshot of every state slice `current_health` derives
/// from. Constructed by `commands::get_health` from `AppState`,
/// `Config`, the secret store, and `sysinfo`. Keeping it a separate
/// struct keeps the derivation pure and testable.
#[derive(Debug, Clone)]
pub struct HealthInputs {
    pub now: DateTime<Utc>,
    pub gamelog_discovered_count: usize,
    pub gamelog_override_set: bool,
    pub remote_sync_enabled: bool,
    pub api_url: Option<String>,
    pub access_token: Option<String>,
    pub web_origin: Option<String>,
    pub auth_lost: bool,
    pub email_verified: Option<bool>,
    pub cookie_configured: bool,
    pub sync_last_error: Option<String>,
    pub sync_attempts_since_success: u32,
    pub hangar_last_attempt_at: Option<DateTime<Utc>>,
    pub hangar_last_success_at: Option<DateTime<Utc>>,
    pub hangar_last_skip_reason: Option<String>,
    pub tail_current_path: Option<String>,
    pub tail_last_event_at: Option<DateTime<Utc>>,
    pub sc_process_running: bool,
    pub disk_free_bytes: Option<u64>,
    pub update_available_version: Option<String>,
    pub dismissed: Vec<DismissedHealth>,
}

/// Derive the ordered list of HealthItems from a snapshot. Pure — no I/O.
pub fn current_health(_inputs: &HealthInputs) -> Vec<HealthItem> {
    Vec::new()
}

fn dismissible_for(severity: Severity) -> bool {
    matches!(severity, Severity::Warn | Severity::Info)
}

fn fingerprint(id: HealthId, params: &HealthParams) -> String {
    // Canonical JSON serialisation of (id, params) — stable across runs
    // and human-readable in the persisted config.
    serde_json::to_string(&(id, params)).unwrap_or_else(|_| String::from("invalid"))
}
```

- [ ] **Step 2.2: Add a fingerprint stability test**

Inside the existing `#[cfg(test)] mod tests` block, append:

```rust
    #[test]
    fn fingerprint_is_stable_for_same_params() {
        let a = fingerprint(HealthId::SyncFailing, &HealthParams::SyncFailing {
            last_error: "502 Bad Gateway".into(),
            attempts_since_success: 3,
        });
        let b = fingerprint(HealthId::SyncFailing, &HealthParams::SyncFailing {
            last_error: "502 Bad Gateway".into(),
            attempts_since_success: 3,
        });
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_differs_when_params_change() {
        let a = fingerprint(HealthId::SyncFailing, &HealthParams::SyncFailing {
            last_error: "502".into(),
            attempts_since_success: 1,
        });
        let b = fingerprint(HealthId::SyncFailing, &HealthParams::SyncFailing {
            last_error: "401".into(),
            attempts_since_success: 1,
        });
        assert_ne!(a, b);
    }

    fn empty_inputs() -> HealthInputs {
        HealthInputs {
            now: chrono::Utc::now(),
            gamelog_discovered_count: 0,
            gamelog_override_set: false,
            remote_sync_enabled: false,
            api_url: None,
            access_token: None,
            web_origin: None,
            auth_lost: false,
            email_verified: None,
            cookie_configured: false,
            sync_last_error: None,
            sync_attempts_since_success: 0,
            hangar_last_attempt_at: None,
            hangar_last_success_at: None,
            hangar_last_skip_reason: None,
            tail_current_path: None,
            tail_last_event_at: None,
            sc_process_running: false,
            disk_free_bytes: None,
            update_available_version: None,
            dismissed: Vec::new(),
        }
    }

    #[test]
    fn current_health_returns_empty_for_empty_inputs() {
        // With nothing configured and remote_sync disabled, the only
        // item that should *eventually* appear is GamelogMissing (no
        // logs discovered, no override). Until that check is implemented
        // the function returns nothing.
        assert!(current_health(&empty_inputs()).is_empty());
    }
```

- [ ] **Step 2.3: Run tests**

```bash
cargo test -p starstats-client --lib health::
```

Expected: 5 passed, 0 failed.

- [ ] **Step 2.4: Commit**

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): add HealthInputs snapshot and empty current_health()"
```

---

### Task 3: Implement the 11 checks (one TDD cycle per check)

Each sub-task follows the same shape: write the failing test → implement the check → tests pass → commit.

The helper `empty_inputs()` defined in Task 2 is reused throughout. Tests live in the same `#[cfg(test)] mod tests` block.

- [ ] **Step 3.1: Test `GamelogMissing` fires when no logs found AND no override**

Append to `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn gamelog_missing_fires_when_no_logs_and_no_override() {
        let inputs = empty_inputs();
        let items = current_health(&inputs);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, HealthId::GamelogMissing);
        assert_eq!(items[0].severity, Severity::Warn);
        assert!(items[0].dismissible);
        assert_eq!(items[0].action, Some(HealthAction::GoToSettings { field: SettingsField::GamelogPath }));
    }

    #[test]
    fn gamelog_missing_does_not_fire_when_override_set() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::GamelogMissing));
    }

    #[test]
    fn gamelog_missing_does_not_fire_when_logs_discovered() {
        let mut inputs = empty_inputs();
        inputs.gamelog_discovered_count = 1;
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::GamelogMissing));
    }
```

Also, since `HealthId`, `HealthAction`, `SettingsField`, `Severity` need `PartialEq, Eq` derives for the test assertions, they already have them from Task 1. Confirm they compile.

- [ ] **Step 3.2: Run tests to verify they fail**

```bash
cargo test -p starstats-client --lib health::tests::gamelog
```

Expected: 3 tests, 1 fail (`gamelog_missing_fires_when_no_logs_and_no_override`), 2 pass (the "does not fire" cases pass trivially because `current_health` returns empty).

- [ ] **Step 3.3: Implement the check**

Inside `current_health`, replace the body:

```rust
pub fn current_health(inputs: &HealthInputs) -> Vec<HealthItem> {
    let mut items: Vec<HealthItem> = Vec::new();

    if inputs.gamelog_discovered_count == 0 && !inputs.gamelog_override_set {
        items.push(item(
            HealthId::GamelogMissing,
            Severity::Warn,
            HealthParams::GamelogMissing,
            Some(HealthAction::GoToSettings { field: SettingsField::GamelogPath }),
        ));
    }

    items.retain(|i| !is_dismissed(i, &inputs.dismissed));
    items.sort_by_key(|i| (severity_order(i.severity), id_order(i.id)));
    items
}

fn item(id: HealthId, severity: Severity, params: HealthParams, action: Option<HealthAction>) -> HealthItem {
    let fingerprint = fingerprint(id, &params);
    HealthItem {
        id,
        severity,
        params,
        action,
        dismissible: dismissible_for(severity),
        fingerprint,
    }
}

fn is_dismissed(item: &HealthItem, dismissed: &[DismissedHealth]) -> bool {
    dismissed.iter().any(|d| d.id == item.id && d.fingerprint == item.fingerprint)
}

fn severity_order(s: Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warn => 1,
        Severity::Info => 2,
    }
}

fn id_order(id: HealthId) -> u8 {
    // Mirrors declaration order in HealthId.
    match id {
        HealthId::GamelogMissing => 0,
        HealthId::ApiUrlMissing => 1,
        HealthId::PairMissing => 2,
        HealthId::AuthLost => 3,
        HealthId::CookieMissing => 4,
        HealthId::SyncFailing => 5,
        HealthId::HangarSkip => 6,
        HealthId::EmailUnverified => 7,
        HealthId::GameLogStale => 8,
        HealthId::UpdateAvailable => 9,
        HealthId::DiskFreeLow => 10,
    }
}
```

- [ ] **Step 3.4: Run all health tests**

```bash
cargo test -p starstats-client --lib health::
```

Expected: all tests pass (5 from Tasks 1-2 + 3 new = 8 total).

- [ ] **Step 3.5: Commit**

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement GamelogMissing check"
```

- [ ] **Step 3.6: Add `ApiUrlMissing` test + implementation + commit**

Test:

```rust
    #[test]
    fn api_url_missing_fires_when_remote_sync_enabled_and_url_unset() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true; // suppress GamelogMissing for isolation
        inputs.remote_sync_enabled = true;
        let items = current_health(&inputs);
        assert!(items.iter().any(|i| i.id == HealthId::ApiUrlMissing));
        let item = items.iter().find(|i| i.id == HealthId::ApiUrlMissing).unwrap();
        assert_eq!(item.severity, Severity::Warn);
        assert_eq!(item.action, Some(HealthAction::GoToSettings { field: SettingsField::ApiUrl }));
    }

    #[test]
    fn api_url_missing_silent_when_remote_sync_disabled() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::ApiUrlMissing));
    }
```

Implementation: insert after the GamelogMissing block inside `current_health`:

```rust
    if inputs.remote_sync_enabled && inputs.api_url.is_none() {
        items.push(item(
            HealthId::ApiUrlMissing,
            Severity::Warn,
            HealthParams::ApiUrlMissing,
            Some(HealthAction::GoToSettings { field: SettingsField::ApiUrl }),
        ));
    }
```

Run `cargo test -p starstats-client --lib health::`, expect all pass. Commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement ApiUrlMissing check"
```

- [ ] **Step 3.7: Add `PairMissing` test + implementation + commit**

Test:

```rust
    #[test]
    fn pair_missing_fires_when_url_set_but_no_token() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.remote_sync_enabled = true;
        inputs.api_url = Some("https://api.example".into());
        let items = current_health(&inputs);
        assert!(items.iter().any(|i| i.id == HealthId::PairMissing));
    }

    #[test]
    fn pair_missing_silent_without_api_url() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.remote_sync_enabled = true;
        let items = current_health(&inputs);
        // ApiUrlMissing fires instead; PairMissing must not.
        assert!(items.iter().all(|i| i.id != HealthId::PairMissing));
    }
```

Implementation (after ApiUrlMissing):

```rust
    if inputs.remote_sync_enabled && inputs.api_url.is_some() && inputs.access_token.is_none() {
        items.push(item(
            HealthId::PairMissing,
            Severity::Warn,
            HealthParams::PairMissing,
            Some(HealthAction::GoToSettings { field: SettingsField::PairingCode }),
        ));
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement PairMissing check"
```

- [ ] **Step 3.8: Add `AuthLost` test + implementation + commit**

Test:

```rust
    #[test]
    fn auth_lost_fires_as_error_when_flag_set() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.auth_lost = true;
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::AuthLost).unwrap();
        assert_eq!(item.severity, Severity::Error);
        assert!(!item.dismissible);
    }
```

Implementation:

```rust
    if inputs.auth_lost {
        items.push(item(
            HealthId::AuthLost,
            Severity::Error,
            HealthParams::AuthLost,
            Some(HealthAction::GoToSettings { field: SettingsField::PairingCode }),
        ));
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement AuthLost check"
```

- [ ] **Step 3.9: Add `SyncFailing` test + implementation (with AuthLost suppression) + commit**

Tests:

```rust
    #[test]
    fn sync_failing_fires_when_last_error_present_and_not_auth_lost() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.sync_last_error = Some("502 Bad Gateway".into());
        inputs.sync_attempts_since_success = 4;
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::SyncFailing).unwrap();
        assert_eq!(item.severity, Severity::Error);
        match &item.params {
            HealthParams::SyncFailing { last_error, attempts_since_success } => {
                assert_eq!(last_error, "502 Bad Gateway");
                assert_eq!(*attempts_since_success, 4);
            }
            _ => panic!("wrong params variant"),
        }
        assert_eq!(item.action, Some(HealthAction::RetrySync));
    }

    #[test]
    fn sync_failing_suppressed_when_auth_lost() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.auth_lost = true;
        inputs.sync_last_error = Some("401 Unauthorized".into());
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::SyncFailing));
        assert!(items.iter().any(|i| i.id == HealthId::AuthLost));
    }
```

Implementation:

```rust
    if inputs.sync_last_error.is_some() && !inputs.auth_lost {
        let last_error = inputs.sync_last_error.clone().unwrap();
        items.push(item(
            HealthId::SyncFailing,
            Severity::Error,
            HealthParams::SyncFailing {
                last_error,
                attempts_since_success: inputs.sync_attempts_since_success,
            },
            Some(HealthAction::RetrySync),
        ));
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement SyncFailing with AuthLost suppression"
```

- [ ] **Step 3.10: Add `HangarSkip` test + implementation + commit**

Tests:

```rust
    #[test]
    fn hangar_skip_fires_when_never_succeeded() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.hangar_last_skip_reason = Some("cookie missing".into());
        inputs.hangar_last_attempt_at = Some(inputs.now);
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::HangarSkip).unwrap();
        assert_eq!(item.severity, Severity::Warn);
        assert_eq!(item.action, Some(HealthAction::RefreshHangar));
    }

    #[test]
    fn hangar_skip_silent_after_a_prior_success() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.hangar_last_skip_reason = Some("rate limited".into());
        inputs.hangar_last_success_at = Some(inputs.now - chrono::Duration::hours(2));
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::HangarSkip));
    }
```

Implementation:

```rust
    if inputs.hangar_last_skip_reason.is_some() && inputs.hangar_last_success_at.is_none() {
        let reason = inputs.hangar_last_skip_reason.clone().unwrap();
        let since = inputs.hangar_last_attempt_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| inputs.now.to_rfc3339());
        items.push(item(
            HealthId::HangarSkip,
            Severity::Warn,
            HealthParams::HangarSkip { reason, since },
            Some(HealthAction::RefreshHangar),
        ));
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement HangarSkip (never-succeeded gating)"
```

- [ ] **Step 3.11: Add `CookieMissing` test + implementation + commit**

Tests:

```rust
    #[test]
    fn cookie_missing_fires_when_paired_and_hangar_attempted() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.remote_sync_enabled = true;
        inputs.api_url = Some("https://api.example".into());
        inputs.access_token = Some("tok".into());
        inputs.cookie_configured = false;
        inputs.hangar_last_attempt_at = Some(inputs.now);
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::CookieMissing).unwrap();
        assert_eq!(item.severity, Severity::Warn);
        assert!(item.dismissible);
    }

    #[test]
    fn cookie_missing_silent_without_hangar_attempt() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.remote_sync_enabled = true;
        inputs.api_url = Some("https://api.example".into());
        inputs.access_token = Some("tok".into());
        inputs.cookie_configured = false;
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::CookieMissing));
    }
```

Implementation:

```rust
    let paired = inputs.api_url.is_some() && inputs.access_token.is_some();
    let hangar_engaged = inputs.hangar_last_attempt_at.is_some();
    if paired && !inputs.cookie_configured && hangar_engaged {
        items.push(item(
            HealthId::CookieMissing,
            Severity::Warn,
            HealthParams::CookieMissing,
            Some(HealthAction::GoToSettings { field: SettingsField::RsiCookie }),
        ));
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement CookieMissing (paired + hangar-engaged gating)"
```

- [ ] **Step 3.12: Add `EmailUnverified` test + implementation + commit**

Tests:

```rust
    #[test]
    fn email_unverified_fires_when_flag_false_and_web_origin_set() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.email_verified = Some(false);
        inputs.web_origin = Some("https://app.example".into());
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::EmailUnverified).unwrap();
        assert_eq!(item.severity, Severity::Warn);
        match &item.action {
            Some(HealthAction::OpenUrl { url }) => assert_eq!(url, "https://app.example/verify-email"),
            _ => panic!("expected OpenUrl action"),
        }
    }

    #[test]
    fn email_unverified_silent_without_web_origin() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.email_verified = Some(false);
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::EmailUnverified));
    }
```

Implementation:

```rust
    if inputs.email_verified == Some(false) {
        if let Some(origin) = &inputs.web_origin {
            // Only valid http(s) origins; defends against a hostile config.
            if origin.starts_with("http://") || origin.starts_with("https://") {
                let url = format!("{}/verify-email", origin.trim_end_matches('/'));
                items.push(item(
                    HealthId::EmailUnverified,
                    Severity::Warn,
                    HealthParams::EmailUnverified,
                    Some(HealthAction::OpenUrl { url }),
                ));
            }
        }
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement EmailUnverified with web_origin guard"
```

- [ ] **Step 3.13: Add `GameLogStale` test + implementation + commit**

Tests:

```rust
    #[test]
    fn game_log_stale_fires_when_sc_running_and_30min_quiet() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.sc_process_running = true;
        inputs.tail_current_path = Some("C:/SC/Game.log".into());
        inputs.tail_last_event_at = Some(inputs.now - chrono::Duration::minutes(31));
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::GameLogStale).unwrap();
        assert_eq!(item.severity, Severity::Warn);
        assert!(item.action.is_none());
    }

    #[test]
    fn game_log_stale_silent_when_sc_not_running() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.tail_current_path = Some("C:/SC/Game.log".into());
        inputs.tail_last_event_at = Some(inputs.now - chrono::Duration::hours(2));
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::GameLogStale));
    }

    #[test]
    fn game_log_stale_silent_when_recent_event() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.sc_process_running = true;
        inputs.tail_current_path = Some("C:/SC/Game.log".into());
        inputs.tail_last_event_at = Some(inputs.now - chrono::Duration::minutes(5));
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::GameLogStale));
    }
```

Implementation:

```rust
    const GAME_LOG_STALE_MIN: i64 = 30;

    if inputs.sc_process_running
        && inputs.tail_current_path.is_some()
    {
        if let Some(last) = inputs.tail_last_event_at {
            let age_min = (inputs.now - last).num_minutes();
            if age_min >= GAME_LOG_STALE_MIN {
                items.push(item(
                    HealthId::GameLogStale,
                    Severity::Warn,
                    HealthParams::GameLogStale {
                        last_event_at: last.to_rfc3339(),
                    },
                    None,
                ));
            }
        }
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement GameLogStale gated on SC process + 30min"
```

- [ ] **Step 3.14: Add `UpdateAvailable` test + implementation + commit**

Tests:

```rust
    #[test]
    fn update_available_fires_when_version_present() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.update_available_version = Some("0.4.1-beta".into());
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::UpdateAvailable).unwrap();
        assert_eq!(item.severity, Severity::Info);
        match &item.params {
            HealthParams::UpdateAvailable { version } => assert_eq!(version, "0.4.1-beta"),
            _ => panic!("wrong params"),
        }
        assert_eq!(item.action, Some(HealthAction::GoToSettings { field: SettingsField::Updates }));
    }
```

Implementation:

```rust
    if let Some(version) = &inputs.update_available_version {
        items.push(item(
            HealthId::UpdateAvailable,
            Severity::Info,
            HealthParams::UpdateAvailable { version: version.clone() },
            Some(HealthAction::GoToSettings { field: SettingsField::Updates }),
        ));
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement UpdateAvailable check"
```

- [ ] **Step 3.15: Add `DiskFreeLow` test + implementation + commit**

Tests:

```rust
    const ONE_GIB: u64 = 1_073_741_824;

    #[test]
    fn disk_free_low_fires_below_one_gib() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.disk_free_bytes = Some(500 * 1024 * 1024); // 500 MiB
        let items = current_health(&inputs);
        let item = items.iter().find(|i| i.id == HealthId::DiskFreeLow).unwrap();
        assert_eq!(item.severity, Severity::Warn);
        assert!(item.action.is_none());
    }

    #[test]
    fn disk_free_low_silent_above_one_gib() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.disk_free_bytes = Some(2 * ONE_GIB);
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::DiskFreeLow));
    }

    #[test]
    fn disk_free_low_silent_when_unknown() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.disk_free_bytes = None;
        let items = current_health(&inputs);
        assert!(items.iter().all(|i| i.id != HealthId::DiskFreeLow));
    }
```

Implementation (the `ONE_GIB` constant is shared; declare it at module scope below `current_health`):

```rust
const DISK_FREE_LOW_THRESHOLD: u64 = 1_073_741_824; // 1 GiB
```

Add to `current_health`:

```rust
    if let Some(free) = inputs.disk_free_bytes {
        if free < DISK_FREE_LOW_THRESHOLD {
            items.push(item(
                HealthId::DiskFreeLow,
                Severity::Warn,
                HealthParams::DiskFreeLow { free_bytes: free },
                None,
            ));
        }
    }
```

Tests pass, commit:

```bash
git add crates/starstats-client/src/health.rs
git commit -m "feat(health): implement DiskFreeLow (<1GiB)"
```

---

### Task 4: Dismissal re-emergence test (regression guard)

**Files:**
- Modify: `crates/starstats-client/src/health.rs` (tests block only)

- [ ] **Step 4.1: Add re-emergence test**

```rust
    #[test]
    fn dismissed_item_does_not_appear() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.sync_last_error = Some("502 BG".into());
        let raw = current_health(&inputs);
        let target = raw.iter().find(|i| i.id == HealthId::SyncFailing).unwrap().clone();
        inputs.dismissed.push(DismissedHealth {
            id: target.id,
            fingerprint: target.fingerprint.clone(),
            dismissed_at: chrono::Utc::now(),
        });
        let after = current_health(&inputs);
        assert!(after.iter().all(|i| i.id != HealthId::SyncFailing));
    }

    #[test]
    fn dismissed_item_reemerges_when_params_change() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.sync_last_error = Some("502 BG".into());
        let raw = current_health(&inputs);
        let target = raw.iter().find(|i| i.id == HealthId::SyncFailing).unwrap().clone();
        inputs.dismissed.push(DismissedHealth {
            id: target.id,
            fingerprint: target.fingerprint.clone(),
            dismissed_at: chrono::Utc::now(),
        });
        // Same id, different error → different fingerprint → re-emerges.
        inputs.sync_last_error = Some("401 Unauthorized".into());
        let after = current_health(&inputs);
        assert!(after.iter().any(|i| i.id == HealthId::SyncFailing));
    }

    #[test]
    fn ordering_puts_errors_before_warns_before_infos() {
        let mut inputs = empty_inputs();
        inputs.gamelog_override_set = true;
        inputs.auth_lost = true;                       // Error
        inputs.update_available_version = Some("0.4.1".into()); // Info
        inputs.remote_sync_enabled = true;
        inputs.api_url = None;                          // Warn (ApiUrlMissing)
        let items = current_health(&inputs);
        let severities: Vec<_> = items.iter().map(|i| i.severity).collect();
        assert_eq!(severities[0], Severity::Error);
        let last = *severities.last().unwrap();
        assert_eq!(last, Severity::Info);
    }
```

- [ ] **Step 4.2: Run all health tests**

```bash
cargo test -p starstats-client --lib health::
```

Expected: all tests pass.

- [ ] **Step 4.3: Commit**

```bash
git add crates/starstats-client/src/health.rs
git commit -m "test(health): cover dismissal re-emergence and severity ordering"
```

---

### Task 5: Add `dismissed_health` to `Config`

**Files:**
- Modify: `crates/starstats-client/src/config.rs`

- [ ] **Step 5.1: Add the field to `Config`**

In `config.rs`, inside the `pub struct Config { ... }` block, after `pub theme: Theme,`:

```rust
    /// Per-user dismissal log for Health items. Permanent (no
    /// expiry); items re-emerge when the underlying params change
    /// (the fingerprint is over (id, params), not (id) alone).
    /// Only `Severity::Warn` and `Severity::Info` items are
    /// dismissible — the rule is enforced Rust-side in `health.rs`.
    #[serde(default)]
    pub dismissed_health: Vec<crate::health::DismissedHealth>,
```

Update `impl Default for Config`:

```rust
            theme: Theme::default(),
            dismissed_health: Vec::new(),
```

- [ ] **Step 5.2: Run build to confirm serde wires correctly**

```bash
cargo build -p starstats-client
```

Expected: clean build.

- [ ] **Step 5.3: Test that an old config without the field deserialises with default**

Add at the bottom of `config.rs`, inside an existing `#[cfg(test)] mod tests` block (or create one):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_dismissed_health_defaults_empty() {
        // Simulate a TOML written before the field existed.
        let toml = r#"
            gamelog_path = "/tmp/Game.log"
            auto_update_check = false
            release_channel = "alpha"
            debug_logging = false
            theme = "stanton"

            [remote_sync]
            enabled = false
            api_url = "https://api.example"
            claimed_handle = ""
            access_token = ""
            interval_secs = 60
            batch_size = 200
        "#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.dismissed_health.is_empty());
    }
}
```

- [ ] **Step 5.4: Run the test**

```bash
cargo test -p starstats-client --lib config::
```

Expected: pass. (If the existing `config.rs` already has a `tests` mod, merge the new test in; do not declare two.)

- [ ] **Step 5.5: Commit**

```bash
git add crates/starstats-client/src/config.rs
git commit -m "feat(config): add dismissed_health field with serde default"
```

---

### Task 6: `probes.rs` — `check_api_url`

**Files:**
- Create: `crates/starstats-client/src/probes.rs`
- Modify: `crates/starstats-client/src/main.rs` (add `mod probes;`)

- [ ] **Step 6.1: Add `mod probes;` to `main.rs` declarations**

In `main.rs`, alongside `mod health;` from Task 1:

```rust
mod probes;
```

- [ ] **Step 6.2a: Add `hangar::probe_with_cookie` helper FIRST**

(This step is required before 6.2b because `probes.rs` imports `crate::hangar::probe_with_cookie`. See step 6.3 below for the full implementation.)

- [ ] **Step 6.2: Create `probes.rs`** *(after 6.2a / 6.3)*

```rust
//! Synchronous opt-in probes for user-entered configuration.
//!
//! `check_api_url` and `check_rsi_cookie` are *not* polled — they
//! fire on user click from the Settings pane. Each performs a single
//! HTTPS request with a tight timeout and returns a structured
//! result the UI can render inline next to the input that produced
//! it. Neither persists state; they're pure probes.

use std::time::Duration;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ApiUrlCheck {
    pub ok: bool,
    pub status: Option<u16>,
    pub server_version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CookieCheck {
    pub ok: bool,
    pub handle: Option<String>,
    pub error: Option<String>,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Probe the configured StarStats server. `/healthz` is exposed by
/// `crates/starstats-server/src/main.rs:371`; we GET it with a 5s
/// timeout. Success means the URL resolves AND a StarStats server is
/// listening (HTTP 200). Server version is read from the optional
/// `X-Server-Version` header if present.
pub async fn check_api_url(url: String) -> ApiUrlCheck {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return ApiUrlCheck {
            ok: false,
            status: None,
            server_version: None,
            error: Some("Invalid URL — must start with http:// or https://".into()),
        };
    }
    let probe_url = format!("{}/healthz", url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ApiUrlCheck {
                ok: false,
                status: None,
                server_version: None,
                error: Some(format!("Couldn't build HTTP client: {e}")),
            };
        }
    };
    let resp = client.get(&probe_url).send().await;
    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            let server_version = r.headers()
                .get("X-Server-Version")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            if r.status().is_success() {
                ApiUrlCheck { ok: true, status: Some(status), server_version, error: None }
            } else {
                ApiUrlCheck {
                    ok: false,
                    status: Some(status),
                    server_version,
                    error: Some(format!("Server returned HTTP {status}")),
                }
            }
        }
        Err(e) => {
            let kind = if e.is_timeout() { "Timeout — server didn't respond in 5s" }
                else if e.is_connect() { "Couldn't connect — check the URL and your network" }
                else { "Network error" };
            ApiUrlCheck {
                ok: false,
                status: None,
                server_version: None,
                error: Some(format!("{kind}: {e}")),
            }
        }
    }
}

/// Probe an RSI session cookie by issuing one authenticated request
/// against `robertsspaceindustries.com`. Returns the handle if the
/// cookie is valid. Does NOT persist the cookie — the user must
/// explicitly hit Save.
///
/// Implementation choice for the endpoint is deferred to call-site
/// review (see spec §14). This first pass reuses the heavyweight
/// `PLEDGES_URL` because the existing client wiring already handles
/// it (cookie injection, user-agent). A future commit may switch to
/// a lighter endpoint once we've validated one.
pub async fn check_rsi_cookie(cookie: String) -> CookieCheck {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return CookieCheck {
            ok: false,
            handle: None,
            error: Some("Paste your Rsi-Token cookie first".into()),
        };
    }
    // Lazily import to avoid a circular dep at top.
    use crate::hangar;
    match hangar::probe_with_cookie(cookie).await {
        Ok(handle) => CookieCheck { ok: true, handle: Some(handle), error: None },
        Err(e) => CookieCheck {
            ok: false,
            handle: None,
            error: Some(format!("{e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_api_url_rejects_non_http() {
        let r = check_api_url("ftp://example.com".into()).await;
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn check_api_url_rejects_garbage() {
        let r = check_api_url("not even a url".into()).await;
        assert!(!r.ok);
    }

    #[tokio::test]
    async fn check_rsi_cookie_rejects_empty() {
        let r = check_rsi_cookie("".into()).await;
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("Paste"));
    }
}
```

- [ ] **Step 6.3 (= Step 6.2a body): Add `hangar::probe_with_cookie` helper**

Open `crates/starstats-client/src/hangar.rs`. Find the public function surface (around the existing pledges-fetching code). Add a new pub function:

```rust
/// One-shot cookie validation probe. Issues an authenticated GET to
/// the pledges page and returns the handle on success. Does not
/// persist anything. Used by `probes::check_rsi_cookie`.
pub async fn probe_with_cookie(cookie: &str) -> anyhow::Result<String> {
    use anyhow::Context;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let resp = client
        .get(PLEDGES_URL)
        .header(reqwest::header::COOKIE, format!("Rsi-Token={cookie}"))
        .header(reqwest::header::USER_AGENT, "StarStats/probe")
        .send()
        .await
        .context("network error")?;
    if !resp.status().is_success() {
        anyhow::bail!("server returned HTTP {}", resp.status().as_u16());
    }
    let body = resp.text().await.context("read body")?;
    // The pledges page embeds the handle inside an a-tag near
    // `class="account-header"`. Existing parser logic in this module
    // already extracts it; reuse the helper if it exists, or do a
    // minimal extraction here. The exact selector lives in this file.
    let handle = extract_handle(&body)
        .ok_or_else(|| anyhow::anyhow!("cookie accepted but handle not found in response"))?;
    Ok(handle)
}

/// Extract `(handle)` text from the pledges page response. If the
/// existing module already has an equivalent helper, replace this
/// body with a call to it.
fn extract_handle(body: &str) -> Option<String> {
    // Look for the existing pattern the parser uses. As a stable
    // fallback, the pledges page reliably contains `data-handle="<H>"`
    // on the account-header element.
    let needle = "data-handle=\"";
    let start = body.find(needle)? + needle.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
```

> **Note:** Before adding this helper verbatim, search `hangar.rs` for an existing handle-extraction function (likely in the existing pledges-parsing path) and call it instead. Duplicating selector logic is the trap to avoid here. If a suitable helper exists, the body of `extract_handle` becomes a delegate call.

- [ ] **Step 6.4: Run the probe tests**

```bash
cargo test -p starstats-client --lib probes::
```

Expected: 3 passed (the URL-validation tests). The full network probes aren't tested here — they're tested at integration time during smoke verification.

- [ ] **Step 6.5: Commit**

```bash
git add crates/starstats-client/src/probes.rs \
        crates/starstats-client/src/hangar.rs \
        crates/starstats-client/src/main.rs
git commit -m "feat(probes): add check_api_url and check_rsi_cookie"
```

---

### Task 7: Add `update_available` to `AppState`

**Files:**
- Modify: `crates/starstats-client/src/state.rs`
- Modify: `crates/starstats-client/src/main.rs` (populate it after auto-update check)

- [ ] **Step 7.1: Add a `UpdateInfo` type and field**

In `state.rs`, near the top with other supporting types:

```rust
/// Snapshot of the most recent auto-update check, populated at
/// startup when `Config.auto_update_check` is true and also after a
/// manual "Check for updates" click. `None` while no check has run
/// yet or the latest check found no update. Surfaced to the UI via
/// `health::current_health` → `HealthId::UpdateAvailable`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}
```

In the `AppState` struct, after `_launcher_handle`:

```rust
    /// Result of the most recent update check. `Some` when a newer
    /// version is available; cleared/set fresh on each check.
    pub update_available: Arc<parking_lot::Mutex<Option<UpdateInfo>>>,
```

- [ ] **Step 7.2: Initialise the field in `AppState::new` (or wherever AppState is constructed)**

In `main.rs` (or wherever `AppState { ... }` is built — there's typically one site), add:

```rust
            update_available: Arc::new(parking_lot::Mutex::new(None)),
```

Build:

```bash
cargo build -p starstats-client
```

Expected: clean build.

- [ ] **Step 7.3: Populate after the existing auto-update check**

Search `main.rs` for the auto-update wiring (likely a `tauri_plugin_updater` block or a Tauri command in `commands.rs`). At the success branch where the updater reports a newer version available, write to the mutex:

```rust
if let Some(update) = check_result.update {
    state.update_available.lock().replace(UpdateInfo {
        version: update.version.clone(),
        checked_at: chrono::Utc::now(),
    });
}
```

Adjust to the exact updater plugin shape — refer to existing logic in `apps/tray-ui/src/updater.ts` and any Rust counterpart for the precise call. If the auto-update logic lives entirely on the JS side, add a tiny Tauri command `set_update_available(version: String)` invoked from `apps/tray-ui/src/updater.ts` after the check resolves with `available: true`.

- [ ] **Step 7.4: Build and commit**

```bash
cargo build -p starstats-client
git add crates/starstats-client/src/state.rs crates/starstats-client/src/main.rs
git commit -m "feat(state): track update_available on AppState"
```

---

### Task 8: Tauri command wrappers in `commands.rs`

**Files:**
- Modify: `crates/starstats-client/src/commands.rs`
- Modify: `crates/starstats-client/src/main.rs` (register in `invoke_handler!`)

- [ ] **Step 8.1: Add the 4 commands**

At the end of `commands.rs`, before the closing brace of the module (or following the last `#[tauri::command]` function):

```rust
// === Health surface (added 2026-05-16) ===

#[tauri::command]
pub async fn get_health(state: tauri::State<'_, AppState>) -> Result<Vec<crate::health::HealthItem>, String> {
    use crate::health::{HealthInputs, current_health};
    use sysinfo::{System, ProcessRefreshKind, RefreshKind, Disks};

    let now = chrono::Utc::now();

    // Read snapshot from AppState mutexes.
    let tail = state.tail_stats.lock().clone();
    let sync = state.sync_stats.lock().clone();
    let hangar = state.hangar_stats.lock().clone();
    let account = state.account_status.lock().clone();
    let update_available = state.update_available.lock().clone();

    // Load Config from disk. Re-reading on each health poll is fine —
    // Config is a tiny TOML and the poll cadence is 15s.
    let config = crate::config::load().map_err(|e| e.to_string())?;
    let gamelog_override_set = config.gamelog_path.is_some();
    let discovered = crate::discovery::discover();

    // Cookie status from the keychain.
    let cookie_configured = match crate::secret::SecretStore::new(crate::secret::ACCOUNT_RSI_SESSION_COOKIE) {
        Ok(store) => store.get().ok().flatten().is_some(),
        Err(_) => false,
    };

    // sysinfo: SC process + free disk on data dir.
    let sys = System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
    let sc_process_running = sys.processes_by_name("StarCitizen.exe").next().is_some()
        || sys.processes_by_name("StarCitizen").next().is_some();

    let data_dir = crate::config::data_dir().ok();
    let disk_free_bytes = data_dir.and_then(|d| free_bytes_for_path(&d));

    let inputs = HealthInputs {
        now,
        gamelog_discovered_count: discovered.len(),
        gamelog_override_set,
        remote_sync_enabled: config.remote_sync.enabled,
        api_url: config.remote_sync.api_url.clone(),
        access_token: config.remote_sync.access_token.clone(),
        web_origin: config.web_origin.clone(),
        auth_lost: account.auth_lost,
        email_verified: account.email_verified,
        cookie_configured,
        sync_last_error: sync.last_error.clone(),
        sync_attempts_since_success: 0, // populated below if available
        hangar_last_attempt_at: hangar.last_attempt_at,
        hangar_last_success_at: hangar.last_success_at,
        hangar_last_skip_reason: hangar.last_skip_reason.clone(),
        tail_current_path: tail.current_path.clone(),
        tail_last_event_at: tail.last_event_at,
        sc_process_running,
        disk_free_bytes,
        update_available_version: update_available.as_ref().map(|u| u.version.clone()),
        dismissed: config.dismissed_health.clone(),
    };

    Ok(current_health(&inputs))
}

#[tauri::command]
pub async fn dismiss_health(id: crate::health::HealthId) -> Result<(), String> {
    use crate::health::{HealthInputs, current_health, DismissedHealth};
    // Load fresh state to compute the live fingerprint for `id`.
    let mut config = crate::config::Config::load().map_err(|e| e.to_string())?;
    // Build a minimal HealthInputs to find the live item — we only
    // need enough state to evaluate the dismissed item's trigger.
    // We piggy-back on get_health's gathering logic by calling it
    // with a fake AppState? No — simpler: dismiss against the current
    // computed list by re-running the snapshot path.
    let live_items = compute_live_items().map_err(|e| e.to_string())?;
    let target = live_items.iter().find(|i| i.id == id)
        .ok_or_else(|| format!("No live HealthItem with id {:?}", id))?;
    if !target.dismissible {
        return Err(format!("HealthItem {:?} is not dismissible", id));
    }
    config.dismissed_health.push(DismissedHealth {
        id: target.id,
        fingerprint: target.fingerprint.clone(),
        dismissed_at: chrono::Utc::now(),
    });
    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// Helper: re-runs the get_health logic but without needing AppState
/// (used by dismiss_health). Implemented as a thin wrapper that
/// returns Vec<HealthItem> by re-reading every source. The cost is
/// one extra round of state-reads per dismissal — fine.
fn compute_live_items() -> anyhow::Result<Vec<crate::health::HealthItem>> {
    // For brevity here, this helper is implemented by extracting the
    // body of get_health into a synchronous function that takes the
    // mutexes by clone. During implementation, refactor get_health
    // to call into a shared `snapshot_inputs(state)` plus
    // `current_health(&inputs)`, then call `snapshot_inputs` from
    // both `get_health` and `dismiss_health`.
    anyhow::bail!("implement during refactor; see plan task 8.2")
}

#[tauri::command]
pub async fn check_api_url(url: String) -> Result<crate::probes::ApiUrlCheck, String> {
    Ok(crate::probes::check_api_url(url).await)
}

#[tauri::command]
pub async fn check_rsi_cookie(cookie: String) -> Result<crate::probes::CookieCheck, String> {
    Ok(crate::probes::check_rsi_cookie(cookie).await)
}

/// Best-effort free-space query for the partition containing `path`.
/// Returns `None` on platforms or paths where it fails.
fn free_bytes_for_path(path: &std::path::Path) -> Option<u64> {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    disks.iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}
```

- [ ] **Step 8.2: Refactor for `compute_live_items`**

The placeholder `compute_live_items` above must not ship — it `bail!`s. Replace with the proper extraction:

1. In `get_health`, move the body that builds `HealthInputs` into a new free function:

```rust
fn snapshot_health_inputs(state: &AppState) -> Result<crate::health::HealthInputs, String> {
    // ... the same body as get_health's body, minus the final
    // current_health() call. Returns HealthInputs directly.
}
```

2. `get_health` becomes:

```rust
#[tauri::command]
pub async fn get_health(state: tauri::State<'_, AppState>) -> Result<Vec<crate::health::HealthItem>, String> {
    let inputs = snapshot_health_inputs(&state)?;
    Ok(crate::health::current_health(&inputs))
}
```

3. `dismiss_health` takes `tauri::State<'_, AppState>` too and uses it:

```rust
#[tauri::command]
pub async fn dismiss_health(id: crate::health::HealthId, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let inputs = snapshot_health_inputs(&state)?;
    let live = crate::health::current_health(&inputs);
    let target = live.iter().find(|i| i.id == id)
        .ok_or_else(|| format!("No live HealthItem with id {:?}", id))?;
    if !target.dismissible {
        return Err(format!("HealthItem {:?} is not dismissible", id));
    }
    let mut config = crate::config::load().map_err(|e| e.to_string())?;
    config.dismissed_health.push(crate::health::DismissedHealth {
        id: target.id,
        fingerprint: target.fingerprint.clone(),
        dismissed_at: chrono::Utc::now(),
    });
    crate::config::save(&config).map_err(|e| e.to_string())?;
    Ok(())
}
```

Drop the temporary `compute_live_items` stub entirely.

- [ ] **Step 8.3: Register commands in `main.rs` `invoke_handler!`**

In `main.rs`, find the existing `tauri::generate_handler!` block (used by `tauri::Builder::default().invoke_handler(...)`). It already lists `commands::get_status`, `commands::get_config`, etc. Add the four new ones at the bottom, alphabetised within the addition:

```rust
            commands::check_api_url,
            commands::check_rsi_cookie,
            commands::dismiss_health,
            commands::get_health,
```

- [ ] **Step 8.4: Build**

```bash
cargo build -p starstats-client
```

Expected: clean build. Common failure modes here:
- `crate::config::user_data_dir` may not exist under that name — locate the equivalent helper in `config.rs` and adjust.
- `sysinfo::Disks` import path may need `use sysinfo::Disks;` at the top of `commands.rs`.

Fix and re-build until clean.

- [ ] **Step 8.5: Commit**

```bash
git add crates/starstats-client/src/commands.rs crates/starstats-client/src/main.rs
git commit -m "feat(commands): wire 4 new Tauri commands for health surface"
```

---

## Phase 2 — Frontend types and Vitest setup

### Task 9: Add Vitest to `apps/tray-ui`

**Files:**
- Modify: `apps/tray-ui/package.json`
- Create: `apps/tray-ui/vitest.config.ts`
- Create: `apps/tray-ui/src/test/setup.ts`

- [ ] **Step 9.1: Install Vitest and testing-library**

```bash
cd apps/tray-ui
pnpm add -D vitest@^1.6.0 jsdom@^24.0.0 @testing-library/react@^16.0.0 @testing-library/jest-dom@^6.4.0 @testing-library/user-event@^14.5.0
```

Expected: deps installed, lockfile updated. If the workspace uses `pnpm`, the command runs from the tray-ui dir. If it uses npm, swap to `npm install -D ...`.

- [ ] **Step 9.2: Add test scripts to `package.json`**

In `apps/tray-ui/package.json`'s `"scripts"`:

```json
    "test": "vitest",
    "test:run": "vitest run"
```

- [ ] **Step 9.3: Create `vitest.config.ts`**

```ts
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
```

- [ ] **Step 9.4: Create `src/test/setup.ts` with a Tauri invoke mock**

```ts
import '@testing-library/jest-dom/vitest';
import { vi } from 'vitest';

// Tauri's invoke() is mocked per-test. Default behaviour rejects so
// any test that forgets to mock a call gets a loud failure rather
// than a silent timeout.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => Promise.reject(new Error('invoke() called without per-test mock'))),
}));
```

- [ ] **Step 9.5: Smoke-test the runner with a trivial test**

Create `apps/tray-ui/src/test/smoke.test.ts`:

```ts
import { describe, it, expect } from 'vitest';

describe('smoke', () => {
  it('runs', () => {
    expect(1 + 1).toBe(2);
  });
});
```

Run:

```bash
pnpm test:run
```

Expected: 1 test passed.

- [ ] **Step 9.6: Delete the smoke file and commit**

```bash
rm apps/tray-ui/src/test/smoke.test.ts
git add apps/tray-ui/package.json apps/tray-ui/pnpm-lock.yaml apps/tray-ui/vitest.config.ts apps/tray-ui/src/test/setup.ts
git commit -m "test(tray-ui): bootstrap Vitest + jsdom + testing-library"
```

---

### Task 10: Frontend TypeScript types in `api.ts`

**Files:**
- Modify: `apps/tray-ui/src/api.ts`

- [ ] **Step 10.1: Add the new types and API methods**

At the bottom of `api.ts` (before `export const api = { ... }`), add:

```ts
// === Health surface ===

export type Severity = 'error' | 'warn' | 'info';

export type HealthId =
  | 'gamelog_missing'
  | 'api_url_missing'
  | 'pair_missing'
  | 'auth_lost'
  | 'cookie_missing'
  | 'sync_failing'
  | 'hangar_skip'
  | 'email_unverified'
  | 'game_log_stale'
  | 'update_available'
  | 'disk_free_low';

export type SettingsField =
  | 'gamelog_path'
  | 'api_url'
  | 'pairing_code'
  | 'rsi_cookie'
  | 'updates';

export type HealthAction =
  | { kind: 'go_to_settings'; field: SettingsField }
  | { kind: 'retry_sync' }
  | { kind: 'refresh_hangar' }
  | { kind: 'open_url'; url: string };

export type HealthParams =
  | { id: 'gamelog_missing' }
  | { id: 'api_url_missing' }
  | { id: 'pair_missing' }
  | { id: 'auth_lost' }
  | { id: 'cookie_missing' }
  | { id: 'sync_failing'; last_error: string; attempts_since_success: number }
  | { id: 'hangar_skip'; reason: string; since: string }
  | { id: 'email_unverified' }
  | { id: 'game_log_stale'; last_event_at: string }
  | { id: 'update_available'; version: string }
  | { id: 'disk_free_low'; free_bytes: number };

export interface HealthItem {
  id: HealthId;
  severity: Severity;
  params: HealthParams;
  action: HealthAction | null;
  dismissible: boolean;
  fingerprint: string;
}

export interface ApiUrlCheck {
  ok: boolean;
  status: number | null;
  server_version: string | null;
  error: string | null;
}

export interface CookieCheck {
  ok: boolean;
  handle: string | null;
  error: string | null;
}
```

- [ ] **Step 10.2: Add the 4 API methods to the `api` object**

Inside `export const api = { ... }`, append:

```ts
  getHealth: () => invoke<HealthItem[]>('get_health'),
  dismissHealth: (id: HealthId) => invoke<void>('dismiss_health', { id }),
  checkApiUrl: (url: string) => invoke<ApiUrlCheck>('check_api_url', { url }),
  checkRsiCookie: (cookie: string) => invoke<CookieCheck>('check_rsi_cookie', { cookie }),
```

- [ ] **Step 10.3: Run typecheck**

```bash
pnpm typecheck
```

Expected: clean.

- [ ] **Step 10.4: Commit**

```bash
git add apps/tray-ui/src/api.ts
git commit -m "feat(api): TypeScript types and methods for health surface"
```

---

## Phase 3 — Frontend pure logic (TDD)

### Task 11: `friendlyError.ts` + tests

**Files:**
- Create: `apps/tray-ui/src/lib/friendlyError.ts`
- Create: `apps/tray-ui/src/lib/friendlyError.test.ts`

- [ ] **Step 11.1: Write the failing test file**

```ts
// apps/tray-ui/src/lib/friendlyError.test.ts
import { describe, it, expect } from 'vitest';
import { friendlyError } from './friendlyError';

describe('friendlyError', () => {
  it('maps timeout strings', () => {
    const e = friendlyError(new Error('request timed out after 5s'));
    expect(e.title).toBe('Timed out');
    expect(e.body).toContain("didn't respond");
  });
  it('maps connection refused / network errors', () => {
    const e = friendlyError(new Error('connection refused: tcp 127.0.0.1:8080'));
    expect(e.title).toBe("Couldn't connect");
  });
  it('maps 401/403 to rejected', () => {
    const e = friendlyError(new Error('server returned HTTP 401'));
    expect(e.title).toBe('Rejected');
    expect(e.hint).toContain('re-pair');
  });
  it('maps 404', () => {
    const e = friendlyError(new Error('endpoint 404 not found'));
    expect(e.title).toBe('Endpoint not found');
  });
  it('maps 5xx', () => {
    const e = friendlyError(new Error('server returned HTTP 502'));
    expect(e.title).toBe('Server error');
  });
  it('maps no-cookie hints', () => {
    const e = friendlyError(new Error('rsi-token cookie missing'));
    expect(e.title).toBe('No RSI cookie');
  });
  it('falls back with 200-char cap', () => {
    const long = 'a'.repeat(300);
    const e = friendlyError(new Error(long));
    expect(e.title).toBe('Something went wrong');
    expect(e.body.endsWith('…')).toBe(true);
    expect(e.body.length).toBeLessThanOrEqual(201);
  });
  it('handles non-Error inputs', () => {
    const e = friendlyError('plain string error');
    expect(e.body).toContain('plain string error');
  });
});
```

- [ ] **Step 11.2: Run the tests to confirm they fail**

```bash
pnpm test:run -- friendlyError
```

Expected: 8 failures, all `Cannot find module './friendlyError'`.

- [ ] **Step 11.3: Implement `friendlyError.ts`**

```ts
export interface FriendlyError {
  title: string;
  body: string;
  hint?: string;
}

const MAX_BODY_LENGTH = 200;

function raw(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

function trim(s: string): string {
  return s.length > MAX_BODY_LENGTH ? s.slice(0, MAX_BODY_LENGTH) + '…' : s;
}

export function friendlyError(err: unknown): FriendlyError {
  const message = raw(err);
  const lower = message.toLowerCase();

  if (/timeout|timed out/.test(lower)) {
    return {
      title: 'Timed out',
      body: "The server didn't respond in time.",
      hint: 'Try again, or check your connection.',
    };
  }
  if (/connection refused|dns|network/.test(lower)) {
    return {
      title: "Couldn't connect",
      body: "Couldn't reach the server.",
      hint: 'Check the API URL or your internet.',
    };
  }
  if (/\b401\b|\b403\b|unauthori[sz]ed/.test(lower)) {
    return {
      title: 'Rejected',
      body: 'The server rejected this device.',
      hint: 'You may need to re-pair.',
    };
  }
  if (/\b404\b|not found/.test(lower)) {
    return {
      title: 'Endpoint not found',
      body: 'The server is up but the endpoint is missing.',
      hint: 'Check the API URL — is it pointing at the right server?',
    };
  }
  if (/\b5\d\d\b/.test(lower)) {
    return {
      title: 'Server error',
      body: 'The server is having problems.',
      hint: 'Try again in a moment.',
    };
  }
  if (/cookie|rsi[-_ ]?token/.test(lower)) {
    return {
      title: 'No RSI cookie',
      body: 'Paste your RSI cookie to enable hangar sync.',
    };
  }
  return {
    title: 'Something went wrong',
    body: trim(message),
    hint: 'Enable Debug logging in Settings to capture details.',
  };
}
```

- [ ] **Step 11.4: Run tests**

```bash
pnpm test:run -- friendlyError
```

Expected: 8 passed.

- [ ] **Step 11.5: Commit**

```bash
git add apps/tray-ui/src/lib/friendlyError.ts apps/tray-ui/src/lib/friendlyError.test.ts
git commit -m "feat(tray-ui): friendlyError mapper with tests"
```

---

### Task 12: `useHealthStrings.ts` + tests

**Files:**
- Create: `apps/tray-ui/src/lib/useHealthStrings.ts`
- Create: `apps/tray-ui/src/lib/useHealthStrings.test.ts`

- [ ] **Step 12.1: Write the failing tests**

```ts
// apps/tray-ui/src/lib/useHealthStrings.test.ts
import { describe, it, expect } from 'vitest';
import { healthStrings } from './useHealthStrings';
import type { HealthParams } from '../api';

describe('healthStrings', () => {
  const variants: HealthParams[] = [
    { id: 'gamelog_missing' },
    { id: 'api_url_missing' },
    { id: 'pair_missing' },
    { id: 'auth_lost' },
    { id: 'cookie_missing' },
    { id: 'sync_failing', last_error: '502 Bad Gateway', attempts_since_success: 3 },
    { id: 'hangar_skip', reason: 'rate limited', since: '2026-05-16T08:00:00Z' },
    { id: 'email_unverified' },
    { id: 'game_log_stale', last_event_at: '2026-05-16T07:00:00Z' },
    { id: 'update_available', version: '0.4.1-beta' },
    { id: 'disk_free_low', free_bytes: 500_000_000 },
  ];

  it.each(variants)('renders a summary for $id', (p) => {
    const out = healthStrings(p);
    expect(out.summary).toBeTruthy();
    expect(out.summary.length).toBeGreaterThan(0);
    expect(out.summary.length).toBeLessThanOrEqual(120);
  });

  it('exhaustively covers every HealthParams variant', () => {
    // If a new variant is added, TypeScript narrowing in healthStrings
    // produces a compile error; this test ensures the variants array
    // here stays in sync (a count mismatch indicates one was missed).
    expect(variants.length).toBe(11);
  });

  it('formats SyncFailing with the error and attempts', () => {
    const out = healthStrings({ id: 'sync_failing', last_error: 'foo', attempts_since_success: 5 });
    expect(out.detail).toContain('foo');
    expect(out.detail).toContain('5');
  });

  it('formats DiskFreeLow as human-readable bytes', () => {
    const out = healthStrings({ id: 'disk_free_low', free_bytes: 500_000_000 });
    expect(out.summary).toMatch(/MB|MiB/);
  });
});
```

- [ ] **Step 12.2: Run tests to confirm they fail**

```bash
pnpm test:run -- useHealthStrings
```

Expected: failures with `Cannot find module`.

- [ ] **Step 12.3: Implement `useHealthStrings.ts`**

```ts
import type { HealthParams } from '../api';

export interface HealthStrings {
  summary: string;
  detail?: string;
}

function fmtBytes(n: number): string {
  if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(0)} MiB`;
  return `${(n / 1024).toFixed(0)} KiB`;
}

function fmtAge(iso: string, now: Date = new Date()): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return iso;
  const diffMin = Math.max(0, Math.round((now.getTime() - t) / 60000));
  if (diffMin < 60) return `${diffMin} min ago`;
  const h = Math.round(diffMin / 60);
  if (h < 48) return `${h} h ago`;
  return `${Math.round(h / 24)} d ago`;
}

export function healthStrings(p: HealthParams): HealthStrings {
  switch (p.id) {
    case 'gamelog_missing':
      return {
        summary: 'No Game.log found — set a path in Settings to start the feed.',
      };
    case 'api_url_missing':
      return {
        summary: 'Remote sync is on but no API URL is set.',
      };
    case 'pair_missing':
      return {
        summary: 'This device isn’t paired with the StarStats server yet.',
      };
    case 'auth_lost':
      return {
        summary: 'This device is no longer paired — re-pair to resume syncing.',
      };
    case 'cookie_missing':
      return {
        summary: 'Hangar sync needs your RSI session cookie — paste it in Settings.',
      };
    case 'sync_failing':
      return {
        summary: 'Remote sync is failing.',
        detail: `${p.last_error} (${p.attempts_since_success} attempts since last success)`,
      };
    case 'hangar_skip':
      return {
        summary: 'Hangar sync skipped.',
        detail: `${p.reason} · ${fmtAge(p.since)}`,
      };
    case 'email_unverified':
      return {
        summary: 'Your Comm-Link email isn’t verified.',
      };
    case 'game_log_stale':
      return {
        summary: 'Game.log has been quiet while Star Citizen is running.',
        detail: `last event ${fmtAge(p.last_event_at)}`,
      };
    case 'update_available':
      return {
        summary: `Update available: v${p.version}.`,
      };
    case 'disk_free_low':
      return {
        summary: `Low disk space: ${fmtBytes(p.free_bytes)} free.`,
      };
  }
}
```

- [ ] **Step 12.4: Run tests**

```bash
pnpm test:run -- useHealthStrings
```

Expected: all pass.

- [ ] **Step 12.5: Commit**

```bash
git add apps/tray-ui/src/lib/useHealthStrings.ts apps/tray-ui/src/lib/useHealthStrings.test.ts
git commit -m "feat(tray-ui): healthStrings mapper with exhaustive tests"
```

---

## Phase 4 — Frontend hooks

### Task 13: `useFieldFocus.tsx` — cross-pane field focus

**Files:**
- Create: `apps/tray-ui/src/hooks/useFieldFocus.tsx`

- [ ] **Step 13.1: Implement**

```tsx
import { createContext, useCallback, useContext, useRef, type ReactNode } from 'react';
import type { SettingsField } from '../api';

type FieldRefMap = Partial<Record<SettingsField, HTMLElement | null>>;

interface FieldFocusContext {
  register: (field: SettingsField, el: HTMLElement | null) => void;
  focus: (field: SettingsField) => void;
}

const Ctx = createContext<FieldFocusContext | null>(null);

export function FieldFocusProvider({ children }: { children: ReactNode }) {
  const refs = useRef<FieldRefMap>({});

  const register = useCallback((field: SettingsField, el: HTMLElement | null) => {
    refs.current[field] = el;
  }, []);

  const focus = useCallback((field: SettingsField) => {
    // The caller typically does `setView('settings')` immediately
    // before calling focus(). React has to commit + mount
    // SettingsPane before our ref is registered, so we retry on
    // animation frames up to a small bound. Bounded so a missing
    // ref doesn't busy-loop forever.
    let attempts = 0;
    const try_focus = () => {
      const el = refs.current[field];
      if (el) {
        el.scrollIntoView({ behavior: 'smooth', block: 'center' });
        const input = el.querySelector<HTMLElement>('input, select, textarea, button');
        (input ?? el).focus();
        return;
      }
      if (attempts++ < 10) {
        window.requestAnimationFrame(try_focus);
      }
    };
    window.requestAnimationFrame(try_focus);
  }, []);

  return <Ctx.Provider value={{ register, focus }}>{children}</Ctx.Provider>;
}

export function useFieldFocus(): FieldFocusContext {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('useFieldFocus must be used inside <FieldFocusProvider>');
  return ctx;
}
```

- [ ] **Step 13.2: Typecheck**

```bash
pnpm typecheck
```

Expected: clean.

- [ ] **Step 13.3: Commit**

```bash
git add apps/tray-ui/src/hooks/useFieldFocus.tsx
git commit -m "feat(tray-ui): useFieldFocus hook for cross-pane navigation"
```

---

### Task 14: `useHealth.ts` polling hook

**Files:**
- Create: `apps/tray-ui/src/hooks/useHealth.ts`

- [ ] **Step 14.1: Implement**

```ts
import { useEffect, useState } from 'react';
import { api, type HealthItem } from '../api';

const POLL_MS = 15_000;

/**
 * Polls `get_health` on a 15s cadence (matching the existing status
 * poll). Returns the most-recent successful list plus a refresh
 * function. Failures are silenced — the Health UI is non-blocking,
 * and per-call surfacing would be noisy. The next successful poll
 * supersedes the silent failure.
 */
export function useHealth(): { items: HealthItem[]; refresh: () => Promise<void> } {
  const [items, setItems] = useState<HealthItem[]>([]);

  const refresh = async () => {
    try {
      const next = await api.getHealth();
      setItems(next);
    } catch {
      // intentionally swallowed; see fn doc
    }
  };

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      if (cancelled) return;
      await refresh();
    })();
    const handle = window.setInterval(() => {
      if (!cancelled) void refresh();
    }, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(handle);
    };
  }, []);

  return { items, refresh };
}
```

- [ ] **Step 14.2: Typecheck and commit**

```bash
pnpm typecheck
git add apps/tray-ui/src/hooks/useHealth.ts
git commit -m "feat(tray-ui): useHealth polling hook"
```

---

## Phase 5 — Frontend components

### Task 15: `HealthCard.tsx` + tests

**Files:**
- Create: `apps/tray-ui/src/components/HealthCard.tsx`
- Create: `apps/tray-ui/src/components/HealthCard.test.tsx`

- [ ] **Step 15.1: Write the failing tests**

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { HealthCard } from './HealthCard';
import { FieldFocusProvider } from '../hooks/useFieldFocus';
import type { HealthItem } from '../api';

const mockGoToSettings = vi.fn();
const mockOnDismiss = vi.fn();

function wrap(ui: React.ReactNode) {
  return render(<FieldFocusProvider>{ui}</FieldFocusProvider>);
}

function makeItem(over: Partial<HealthItem> = {}): HealthItem {
  return {
    id: 'api_url_missing',
    severity: 'warn',
    params: { id: 'api_url_missing' },
    action: { kind: 'go_to_settings', field: 'api_url' },
    dismissible: true,
    fingerprint: 'fp-1',
    ...over,
  };
}

describe('HealthCard', () => {
  beforeEach(() => {
    mockGoToSettings.mockReset();
    mockOnDismiss.mockReset();
  });

  it('renders nothing when items is empty', () => {
    const { container } = wrap(<HealthCard items={[]} onGoToSettings={mockGoToSettings} onDismiss={mockOnDismiss} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders one row per item', () => {
    wrap(
      <HealthCard
        items={[
          makeItem({ id: 'api_url_missing' }),
          makeItem({ id: 'gamelog_missing', params: { id: 'gamelog_missing' } }),
        ]}
        onGoToSettings={mockGoToSettings}
        onDismiss={mockOnDismiss}
      />
    );
    expect(screen.getAllByRole('listitem')).toHaveLength(2);
  });

  it('shows Dismiss only when dismissible', () => {
    wrap(
      <HealthCard
        items={[
          makeItem({ severity: 'error', dismissible: false, params: { id: 'auth_lost' }, id: 'auth_lost' }),
          makeItem({ severity: 'info', dismissible: true, params: { id: 'update_available', version: '0.4.1' }, id: 'update_available' }),
        ]}
        onGoToSettings={mockGoToSettings}
        onDismiss={mockOnDismiss}
      />
    );
    expect(screen.getAllByRole('button', { name: /dismiss/i })).toHaveLength(1);
  });

  it('clicking a go_to_settings CTA calls onGoToSettings with the field', async () => {
    const user = userEvent.setup();
    wrap(
      <HealthCard
        items={[makeItem()]}
        onGoToSettings={mockGoToSettings}
        onDismiss={mockOnDismiss}
      />
    );
    await user.click(screen.getByRole('button', { name: /set up|fix|go/i }));
    expect(mockGoToSettings).toHaveBeenCalledWith('api_url');
  });

  it('clicking Dismiss calls onDismiss with the item id', async () => {
    const user = userEvent.setup();
    wrap(
      <HealthCard
        items={[makeItem({ id: 'cookie_missing', params: { id: 'cookie_missing' } })]}
        onGoToSettings={mockGoToSettings}
        onDismiss={mockOnDismiss}
      />
    );
    await user.click(screen.getByRole('button', { name: /dismiss/i }));
    expect(mockOnDismiss).toHaveBeenCalledWith('cookie_missing');
  });
});
```

- [ ] **Step 15.2: Implement `HealthCard.tsx`**

```tsx
import type { HealthItem, HealthId, SettingsField } from '../api';
import { healthStrings } from '../lib/useHealthStrings';
import { TrayCard, GhostButton, StatusDot } from './tray/primitives';

interface Props {
  items: HealthItem[];
  onGoToSettings: (field: SettingsField) => void;
  onDismiss: (id: HealthId) => void;
  onRetrySync?: () => void;
  onRefreshHangar?: () => void;
  onOpenUrl?: (url: string) => void;
}

const SEVERITY_TONE: Record<HealthItem['severity'], 'danger' | 'warn' | 'info'> = {
  error: 'danger',
  warn: 'warn',
  info: 'info',
};

const CTA_LABEL: Record<string, string> = {
  go_to_settings: 'Set up',
  retry_sync: 'Retry sync',
  refresh_hangar: 'Refresh now',
  open_url: 'Open',
};

export function HealthCard({
  items,
  onGoToSettings,
  onDismiss,
  onRetrySync,
  onRefreshHangar,
  onOpenUrl,
}: Props) {
  if (items.length === 0) return null;

  return (
    <TrayCard
      title="Health"
      kicker={`${items.length} issue${items.length === 1 ? '' : 's'}`}
    >
      <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
        {items.map((it) => {
          const strings = healthStrings(it.params);
          return (
            <li
              key={`${it.id}:${it.fingerprint}`}
              style={{
                display: 'grid',
                gridTemplateColumns: 'auto 1fr auto auto',
                alignItems: 'center',
                gap: 10,
                padding: '6px 8px',
                background: 'var(--surface-2)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--r-sm)',
              }}
            >
              <StatusDot tone={SEVERITY_TONE[it.severity]} />
              <div style={{ display: 'flex', flexDirection: 'column' }}>
                <span style={{ fontSize: 13, color: 'var(--fg)' }}>{strings.summary}</span>
                {strings.detail && (
                  <span style={{ fontSize: 11, color: 'var(--fg-dim)' }}>{strings.detail}</span>
                )}
              </div>
              {it.action && (
                <GhostButton
                  type="button"
                  onClick={() => {
                    const a = it.action!;
                    switch (a.kind) {
                      case 'go_to_settings':
                        onGoToSettings(a.field);
                        break;
                      case 'retry_sync':
                        onRetrySync?.();
                        break;
                      case 'refresh_hangar':
                        onRefreshHangar?.();
                        break;
                      case 'open_url':
                        onOpenUrl?.(a.url);
                        break;
                    }
                  }}
                  style={{ padding: '3px 9px', fontSize: 11 }}
                >
                  {CTA_LABEL[it.action.kind] ?? 'Fix'}
                </GhostButton>
              )}
              {it.dismissible ? (
                <GhostButton
                  type="button"
                  onClick={() => onDismiss(it.id)}
                  style={{ padding: '3px 9px', fontSize: 11 }}
                  aria-label={`Dismiss ${it.id}`}
                >
                  Dismiss
                </GhostButton>
              ) : (
                <span />
              )}
            </li>
          );
        })}
      </ul>
    </TrayCard>
  );
}
```

- [ ] **Step 15.3: Run tests**

```bash
pnpm test:run -- HealthCard
```

Expected: all pass.

- [ ] **Step 15.4: Commit**

```bash
git add apps/tray-ui/src/components/HealthCard.tsx apps/tray-ui/src/components/HealthCard.test.tsx
git commit -m "feat(tray-ui): HealthCard component with tests"
```

---

### Task 16: `InlineCheck.tsx` + tests

**Files:**
- Create: `apps/tray-ui/src/components/InlineCheck.tsx`
- Create: `apps/tray-ui/src/components/InlineCheck.test.tsx`

- [ ] **Step 16.1: Write the failing tests**

```tsx
import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { InlineCheck } from './InlineCheck';

describe('InlineCheck', () => {
  it('is disabled when input is empty', () => {
    render(<InlineCheck label="Test" value="" onCheck={vi.fn()} />);
    expect(screen.getByRole('button', { name: /test/i })).toBeDisabled();
  });

  it('shows a success line after a successful check', async () => {
    const user = userEvent.setup();
    const onCheck = vi.fn().mockResolvedValue({ ok: true, message: 'reachable' });
    render(<InlineCheck label="Test" value="https://x" onCheck={onCheck} />);
    await user.click(screen.getByRole('button', { name: /test/i }));
    await waitFor(() => expect(screen.getByText(/reachable/i)).toBeInTheDocument());
  });

  it('shows an error line after a failed check', async () => {
    const user = userEvent.setup();
    const onCheck = vi.fn().mockResolvedValue({ ok: false, message: 'HTTP 404' });
    render(<InlineCheck label="Test" value="https://x" onCheck={onCheck} />);
    await user.click(screen.getByRole('button', { name: /test/i }));
    await waitFor(() => expect(screen.getByText(/HTTP 404/)).toBeInTheDocument());
  });

  it('disables the button while running', async () => {
    const user = userEvent.setup();
    const onCheck = vi.fn(() => new Promise(() => {})); // never resolves
    render(<InlineCheck label="Test" value="https://x" onCheck={onCheck} />);
    await user.click(screen.getByRole('button', { name: /test/i }));
    expect(screen.getByRole('button', { name: /testing|test/i })).toBeDisabled();
  });
});
```

- [ ] **Step 16.2: Implement**

```tsx
import { useState } from 'react';
import { GhostButton } from './tray/primitives';

export interface InlineCheckResult {
  ok: boolean;
  message: string;
}

interface Props {
  label: string;
  value: string;
  onCheck: (value: string) => Promise<InlineCheckResult>;
}

type State =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'ok'; detail: string }
  | { kind: 'err'; detail: string };

export function InlineCheck({ label, value, onCheck }: Props) {
  const [state, setState] = useState<State>({ kind: 'idle' });
  const disabled = state.kind === 'running' || !value.trim();

  const run = async () => {
    setState({ kind: 'running' });
    try {
      const r = await onCheck(value);
      setState(r.ok ? { kind: 'ok', detail: r.message } : { kind: 'err', detail: r.message });
    } catch (e) {
      setState({ kind: 'err', detail: e instanceof Error ? e.message : String(e) });
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginTop: 6 }}>
      <GhostButton
        type="button"
        onClick={run}
        disabled={disabled}
        style={{ padding: '3px 10px', fontSize: 11, alignSelf: 'flex-start' }}
      >
        {state.kind === 'running' ? `Testing…` : label}
      </GhostButton>
      {state.kind === 'ok' && (
        <span style={{ fontSize: 11, color: 'var(--ok)' }}>✓ {state.detail}</span>
      )}
      {state.kind === 'err' && (
        <span style={{ fontSize: 11, color: 'var(--danger)' }}>✗ {state.detail}</span>
      )}
    </div>
  );
}
```

- [ ] **Step 16.3: Run tests**

```bash
pnpm test:run -- InlineCheck
```

Expected: all pass.

- [ ] **Step 16.4: Commit**

```bash
git add apps/tray-ui/src/components/InlineCheck.tsx apps/tray-ui/src/components/InlineCheck.test.tsx
git commit -m "feat(tray-ui): InlineCheck component with tests"
```

---

## Phase 6 — Integration

### Task 17: Wire `FieldFocusProvider` into `App.tsx`

**Files:**
- Modify: `apps/tray-ui/src/App.tsx`

- [ ] **Step 17.1: Import and wrap**

In `App.tsx` near the top:

```tsx
import { FieldFocusProvider } from './hooks/useFieldFocus';
```

Replace the outer `<div className="app">…</div>` with:

```tsx
<FieldFocusProvider>
  <div className="app">
    {/* …existing children… */}
  </div>
</FieldFocusProvider>
```

- [ ] **Step 17.2: Typecheck**

```bash
pnpm typecheck
```

Expected: clean.

- [ ] **Step 17.3: Commit**

```bash
git add apps/tray-ui/src/App.tsx
git commit -m "feat(tray-ui): wrap app in FieldFocusProvider"
```

---

### Task 18: Render `HealthCard` in `StatusPane.tsx` and remove old banners

**Files:**
- Modify: `apps/tray-ui/src/components/StatusPane.tsx`

- [ ] **Step 18.1: Import the new pieces**

```tsx
import { HealthCard } from './HealthCard';
import { useHealth } from '../hooks/useHealth';
import { useFieldFocus } from '../hooks/useFieldFocus';
```

- [ ] **Step 18.2: Update `Props` to drop `onGoToSettings`**

Replace the existing `Props` interface — the `onGoToSettings` callback used by the (now-removed) auth-lost banner moves into the HealthCard via `useFieldFocus`. The `StatusPane` no longer needs the callback.

```tsx
interface Props {
  status: StatusResponse;
  webOrigin: string | null;
}
```

Update the parent in `App.tsx`:

```tsx
<StatusPane status={status} webOrigin={...} />
```

(remove the `onGoToSettings` prop).

But the HealthCard *does* need a way to ask App.tsx to switch view to Settings. Two options:

- (a) HealthCard receives an `onGoToSettings` prop and StatusPane forwards it from App.
- (b) HealthCard uses `useFieldFocus` plus a separate context for view-switching.

Use option (a) — pass `onGoToSettings: (field) => void` down through `StatusPane → HealthCard`. App.tsx is the owner; it does `setView('settings')` and then `fieldFocus.focus(field)`. Update the prop on `StatusPane`:

```tsx
interface Props {
  status: StatusResponse;
  webOrigin: string | null;
  onGoToSettings: (field: SettingsField) => void;
}
```

…and forward to `<HealthCard onGoToSettings={onGoToSettings} ...>`.

- [ ] **Step 18.3: Add `HealthCard` rendering at the top of the pane**

Inside the existing top-level `<div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>`, BEFORE the existing `{showAuthLost && (...)}` and `{showEmailUnverified && (...)}` blocks (which we'll remove), add:

```tsx
const { items: healthItems, refresh: refreshHealth } = useHealth();
// ...
<HealthCard
  items={healthItems}
  onGoToSettings={onGoToSettings}
  onDismiss={async (id) => {
    await api.dismissHealth(id);
    void refreshHealth();
  }}
  onRetrySync={async () => {
    await api.retrySyncNow();
    void refreshHealth();
  }}
  onRefreshHangar={async () => {
    await api.refreshHangarNow();
    void refreshHealth();
  }}
  onOpenUrl={async (url) => {
    const { open } = await import('@tauri-apps/plugin-shell');
    await open(url);
  }}
/>
```

- [ ] **Step 18.4: Remove the obsolete banners**

Delete the `{showAuthLost && (<Banner ...>)}` JSX block and the `{showEmailUnverified && (<Banner ...>)}` JSX block. Also remove their derived booleans (`showAuthLost`, `showEmailUnverified`) and any locally-unused helpers (e.g., `isSafeWebOrigin` if no other use remains). The `Banner` import can also go if nothing else uses it; otherwise keep.

- [ ] **Step 18.5: Update `App.tsx` to plumb `onGoToSettings`**

In `App.tsx`:

```tsx
import { useFieldFocus } from './hooks/useFieldFocus';
// inside App():
const fieldFocus = useFieldFocus(); // wait — this is at the wrong layer
```

Actually `useFieldFocus()` requires being inside the provider. Re-shape: do the navigation directly in App.tsx via a callback passed *into* the JSX where the provider is already wrapping. Approach:

```tsx
import { FieldFocusProvider, useFieldFocus } from './hooks/useFieldFocus';

// Inner component that lives inside the provider.
function AppInner() {
  const [view, setView] = useState<TrayView>('status');
  // ... existing state ...
  const fieldFocus = useFieldFocus();

  const onGoToSettings = (field: SettingsField) => {
    setView('settings');
    fieldFocus.focus(field);
  };

  return (
    <div className="app">
      <TrayHeader ... />
      <main className="app__main">
        {view === 'status' && status && (
          <StatusPane status={status} webOrigin={...} onGoToSettings={onGoToSettings} />
        )}
        {view === 'logs' && <LogsPane />}
        {view === 'settings' && config && (
          <SettingsPane config={config} onSave={onSaveConfig} />
        )}
      </main>
    </div>
  );
}

export default function App() {
  return (
    <FieldFocusProvider>
      <AppInner />
    </FieldFocusProvider>
  );
}
```

- [ ] **Step 18.6: Typecheck**

```bash
pnpm typecheck
```

Expected: clean.

- [ ] **Step 18.7: Commit**

```bash
git add apps/tray-ui/src/App.tsx apps/tray-ui/src/components/StatusPane.tsx
git commit -m "feat(tray-ui): render HealthCard, remove legacy banners, wire onGoToSettings"
```

---

### Task 19: Add `InlineCheck` and field refs to `SettingsPane.tsx`; replace raw errors

**Files:**
- Modify: `apps/tray-ui/src/components/SettingsPane.tsx`

- [ ] **Step 19.1: Import**

```tsx
import { useFieldFocus } from '../hooks/useFieldFocus';
import { InlineCheck, type InlineCheckResult } from './InlineCheck';
import { friendlyError } from '../lib/friendlyError';
```

- [ ] **Step 19.2: Register field refs**

Inside `SettingsPane()`:

```tsx
const fieldFocus = useFieldFocus();
const gamelogPathRef = useRef<HTMLDivElement>(null);
const apiUrlRef = useRef<HTMLDivElement>(null);
const pairingCodeRef = useRef<HTMLDivElement>(null);
const rsiCookieRef = useRef<HTMLDivElement>(null);
const updatesRef = useRef<HTMLDivElement>(null);

useEffect(() => {
  fieldFocus.register('gamelog_path', gamelogPathRef.current);
  fieldFocus.register('api_url', apiUrlRef.current);
  fieldFocus.register('pairing_code', pairingCodeRef.current);
  fieldFocus.register('rsi_cookie', rsiCookieRef.current);
  fieldFocus.register('updates', updatesRef.current);
}, [fieldFocus]);
```

- [ ] **Step 19.3: Attach refs to each section's wrapping element**

For each of the relevant `<Field>` or `<TrayCard>` blocks, add an outer `<div ref={...}>` wrapping (or attach to the existing wrapper):

- `gamelog_path` → wrap the "Game.log" `TrayCard`'s outer div with `gamelogPathRef`.
- `api_url` → wrap the API URL `<Field>` with `apiUrlRef`.
- `pairing_code` → wrap the pairing `<Field>` (the one with the 8-char input) with `pairingCodeRef`.
- `rsi_cookie` → wrap the cookie input `<Field>` with `rsiCookieRef`.
- `updates` → wrap the "Updates" `TrayCard` with `updatesRef`.

- [ ] **Step 19.4: Add InlineCheck under the API URL input**

```tsx
<InlineCheck
  label="Test connection"
  value={draft.remote_sync.api_url ?? ''}
  onCheck={async (url): Promise<InlineCheckResult> => {
    const r = await api.checkApiUrl(url);
    return {
      ok: r.ok,
      message: r.ok
        ? `Reachable${r.server_version ? ` · server v${r.server_version}` : ''}`
        : (r.error ?? 'Unknown error'),
    };
  }}
/>
```

Insert immediately after the existing `<TextInput type="url" ...>` field for API URL.

- [ ] **Step 19.5: Add InlineCheck under the RSI cookie input**

```tsx
<InlineCheck
  label="Test cookie"
  value={cookieDraft}
  onCheck={async (cookie): Promise<InlineCheckResult> => {
    const r = await api.checkRsiCookie(cookie);
    return {
      ok: r.ok,
      message: r.ok
        ? `Authenticated as ${r.handle}`
        : (r.error ?? 'Unknown error'),
    };
  }}
/>
```

Insert immediately after the existing cookie `<TextInput type="password" ...>` field.

- [ ] **Step 19.6: Replace raw error strings with friendlyError**

For each `setX Error(String(err))` in `handlePair`, `handleSaveCookie`, `handleClearCookie`, `handleSubmit`: replace with `friendlyError(err).title + ': ' + friendlyError(err).body` (or render multi-line — easiest is concat for now since the existing error UI is a single span).

Example for `handlePair`:

```tsx
} catch (err) {
  const f = friendlyError(err);
  setPairError(`${f.title}: ${f.body}`);
}
```

Apply the same transformation to all `setCookieError(String(err))`, `setError(String(err))`, and `setRefreshError(...)` sites (in `StatusPane.tsx` too, for the hangar refresh handler).

- [ ] **Step 19.7: Typecheck and run frontend tests**

```bash
pnpm typecheck
pnpm test:run
```

Expected: clean typecheck; all tests pass.

- [ ] **Step 19.8: Commit**

```bash
git add apps/tray-ui/src/components/SettingsPane.tsx apps/tray-ui/src/components/StatusPane.tsx
git commit -m "feat(tray-ui): InlineCheck probes + friendlyError in Settings/Status"
```

---

## Phase 7 — Build verification + push

### Task 20: Full test sweep

- [ ] **Step 20.1: Rust tests**

```bash
cargo test --workspace
```

Expected: all packages pass. Fix any unrelated regressions before moving on (if any appear, the prior tasks introduced them — bisect by re-running cargo test after each prior commit).

- [ ] **Step 20.2: Frontend tests**

```bash
cd apps/tray-ui
pnpm test:run
```

Expected: all tests pass.

- [ ] **Step 20.3: Lint and typecheck**

```bash
pnpm typecheck
pnpm lint
```

Expected: clean.

- [ ] **Step 20.4: Production build**

```bash
pnpm build
cargo build -p starstats-client --release
```

Expected: both succeed. The bundle output goes to the usual Tauri target dir; verify no compilation errors.

---

### Task 21: Code review pass

- [ ] **Step 21.1: Spawn the `pr-review-toolkit:code-reviewer` agent on the branch diff**

Run:

```bash
git diff main...HEAD --stat
```

Expected: lists all modified/added files.

Then dispatch the code-reviewer agent (described in this repo's CLAUDE.md):

```
Agent({
  description: "Branch review for tray health surface",
  subagent_type: "pr-review-toolkit:code-reviewer",
  prompt: "Review the diff between main and feat/tray-health-surface. Focus on:
- the new Rust modules (health.rs, probes.rs) for correctness, error handling, no panics on user-controllable inputs
- commands.rs new entries — verify they're registered in invoke_handler!
- Config schema change — backward compat
- React component patterns — match existing tray-ui style
- friendlyError mapping completeness
Return a punch list of MUST-FIX / SHOULD-FIX / NIT items with file:line refs."
})
```

- [ ] **Step 21.2: Address MUST-FIX and SHOULD-FIX items**

For each issue: fix → re-test → commit with `fix:` prefix. NITs are optional; defer them.

---

### Task 22: Push the branch

- [ ] **Step 22.1: Verify branch state**

```bash
git status
git log --oneline main..HEAD
```

Expected: working tree clean; commit list looks coherent (one-per-task, conventional commit messages).

- [ ] **Step 22.2: Push**

```bash
git push -u origin feat/tray-health-surface
```

Expected: branch published, GitHub returns a URL for opening a PR. Do not auto-create the PR — leave that to the user.

---

## Self-review

**Spec coverage:**
- §2 Architecture (pure health derivation + I/O probes + Tauri wrappers) → Tasks 1-3, 6, 8.
- §3 Data shapes (HealthItem, HealthParams, HealthAction, SettingsField) → Task 1.
- §4 Dismissal (fingerprint canonical JSON, Info+Warn dismissible) → Tasks 2 (fingerprint), 5 (Config field), 8 (dismiss_health command), 15 (Dismiss button rendering).
- §5 The 11 checks → Task 3 (sub-steps 3.1-3.15).
- §6 Inline probes → Task 6.
- §7 Friendly error mapping → Task 11.
- §8 HealthCard placement + interaction → Tasks 15, 17, 18.
- §9 New TrayState field (update_available) → Task 7.
- §10 Dependencies → Task 6 / 9 / module declarations.
- §11 Testing → Rust tests inline per check; frontend tests Tasks 9, 11, 12, 15, 16.
- §12 Migration → Task 5 (serde default), Task 18 (banner removal).

**Placeholder scan:** Step 7.3 references `check_result.update` and `state.update_available` with a "Adjust to the exact updater plugin shape" hedge. This is the one TBD that survives — resolved at implementation time by reading `updater.ts` and the Tauri updater plugin. Acceptable because it's a single, scoped local question, not a structural ambiguity. Step 6.3's `extract_handle` likewise notes "search hangar.rs for an existing handle-extraction function" — also acceptable as the helper already exists in some form.

**Type consistency:** `HealthId`/`Severity`/`HealthParams`/`HealthAction`/`SettingsField` are defined once (Task 1) and referenced consistently. Method names: `getHealth` / `dismissHealth` / `checkApiUrl` / `checkRsiCookie` consistent between Rust commands and TypeScript api object. `healthStrings` (not `useHealthStrings` despite the filename) is a pure function exported from the file named `useHealthStrings.ts` — minor mismatch but intentional (file is named for what it produces; export is a plain function because no hook semantics are needed).

**Scope check:** ~20 tasks, ~80 sub-steps. Large but each task is independent and bite-sized. Suitable for either subagent-driven or inline execution.
