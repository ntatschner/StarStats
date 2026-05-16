# Tray Setup & Health Surface — Design

**Status**: design / pre-implementation
**Date**: 2026-05-16
**Scope**: `apps/tray-ui/` + `crates/starstats-client/`
**Out of scope**: a first-run modal/wizard (deferred), header health pip (deferred), any change to the `/v1/events` ingestion API, on-disk schema, or release-manifest format.

## 1. Problem

The current tray surfaces failure modes in scattered places — a banner at the top for `auth_lost`, an inline error inside the Hangar card for skip reasons, a per-field error inside Settings for pair/cookie save failures, an empty-state "Scope is clear" message for missing `Game.log`. A user (including the maintainer as daily driver) whose pipeline is broken in two places has to assemble a mental model by scrolling. Configuration steps (API URL, RSI cookie, pairing) are also fire-and-forget: type, save, wait for a later async cycle to (maybe) report failure. There is no way to ask "is this thing I just typed actually working?" before depending on it.

Setup-time and lifetime audiences overlap in *what they need*: a single legible "here is what is wrong and what to do about it" view, plus inline validation of inputs at the point they're entered. This spec unifies both behind one component (`HealthCard`) backed by one Rust module (`health.rs`) plus two opt-in probes (`probes.rs`).

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│  Rust  (crates/starstats-client/src/)                                │
│                                                                       │
│  health.rs   ◀── NEW (pure derivation)                                │
│    pub fn current_health(state: &TrayState) -> Vec<HealthItem>        │
│        Reads: Config, StatusSnapshot, RsiCookieStatus, last sync     │
│        error, sysinfo for SC process + free disk.                     │
│        No background work, no new persistence.                        │
│                                                                       │
│  probes.rs   ◀── NEW (I/O)                                            │
│    pub async fn check_api_url(url: String) -> ApiUrlCheck             │
│    pub async fn check_rsi_cookie(cookie: String) -> CookieCheck       │
│        Both perform one HTTPS request with a 5s timeout. Neither      │
│        persists anything; pure probes.                                │
│                                                                       │
│  commands.rs   ◀── 4 new Tauri commands (thin wrappers)               │
│    get_health()              → Vec<HealthItem>                        │
│    dismiss_health(id)        → ()                                     │
│    check_api_url(url)        → ApiUrlCheck                            │
│    check_rsi_cookie(cookie)  → CookieCheck                            │
└──────────────────────────────────┬──────────────────────────────────┘
                                   │ Tauri IPC
┌──────────────────────────────────▼──────────────────────────────────┐
│  React  (apps/tray-ui/src/)                                          │
│                                                                       │
│  hooks/useHealth.ts          ◀── NEW                                  │
│  hooks/useFieldFocus.ts      ◀── NEW (coordinates Settings field     │
│                                       focus from cross-pane CTAs)    │
│  lib/friendlyError.ts        ◀── NEW                                  │
│  lib/useHealthStrings.ts     ◀── NEW (HealthParams → user strings)   │
│  components/HealthCard.tsx   ◀── NEW                                  │
│  components/InlineCheck.tsx  ◀── NEW                                  │
└─────────────────────────────────────────────────────────────────────┘
```

`current_health()` is a **pure function over already-collected state**. The only I/O the design adds is the opt-in `check_*` probes, which fire on user click rather than on a timer. `sysinfo` is consulted synchronously inside `current_health()` for SC process detection and free-disk-space — both cheap calls.

## 3. Data shapes

```rust
// crates/starstats-client/src/health.rs

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HealthItem {
    pub id: HealthId,
    pub severity: Severity,
    pub params: HealthParams,
    pub action: Option<HealthAction>,
    pub dismissible: bool,   // derived from severity, authoritative Rust-side
    pub fingerprint: String, // hash of (id, params); used for dismissal re-emergence
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Severity { Error, Warn, Info }

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "id", rename_all = "snake_case")]
pub enum HealthParams {
    GamelogMissing,
    ApiUrlMissing,
    PairMissing,
    AuthLost,
    CookieMissing,
    SyncFailing       { last_error: String, attempts_since_success: u32 },
    HangarSkip        { reason: String, since: String },
    EmailUnverified,
    GameLogStale      { last_event_at: String },
    UpdateAvailable   { version: String },
    DiskFreeLow       { free_bytes: u64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthAction {
    GoToSettings { field: SettingsField },
    RetrySync,
    RefreshHangar,
    OpenUrl { url: String },  // http(s)-only, validated Rust-side at construction
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsField {
    GamelogPath,
    ApiUrl,
    PairingCode,
    RsiCookie,
    Updates,
}
```

**Design notes:**

- `HealthId` is a closed enum, so adding a check is a deliberate compile-time change.
- `HealthParams` carries the structured payload the React side needs; React owns user-facing strings via `useHealthStrings.ts`. This is i18n-ready and prevents string drift between Rust and tests.
- `dismissible` is computed Rust-side as `matches!(severity, Severity::Warn | Severity::Info)`. The React side renders the Dismiss button strictly from this field; it cannot widen the rule.
- `fingerprint` is `serde_json::to_string(&(id, &params))?` — the canonical JSON of `(id, params)`. Stable across runs by construction (serde's serialisation is deterministic for these types), no hash function required, no new dependency. Storage cost is bounded (params payloads are small; the only unbounded field is `SyncFailing.last_error`, which is itself capped server-side before reaching the tray).
- `HealthAction::OpenUrl` URLs are constructed Rust-side from known-safe origins (e.g., `config.web_origin`) with hard-coded paths. Never echoes user input.

## 4. Dismissal mechanism

New field on `Config`:

```rust
pub struct Config {
    // ...existing fields...
    pub dismissed_health: Vec<DismissedHealth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DismissedHealth {
    pub id: HealthId,
    pub fingerprint: String,
    pub dismissed_at: DateTime<Utc>,
}
```

`current_health()` filters out any item whose `(id, fingerprint)` matches an entry in `dismissed_health`. Because the fingerprint is over `params`, a dismissed `SyncFailing { last_error: "502" }` automatically re-emerges as a fresh item if the error changes to `"401"`. Dismissal is opt-in per fingerprint, not per category.

Only `Severity::Warn` and `Severity::Info` items show a Dismiss button. The Rust enum is the gate; the React `HealthCard` reads `item.dismissible` and renders accordingly.

New command: `dismiss_health(id: HealthId) -> ()`. It looks up the current item with that `id`, appends `{id, fingerprint, dismissed_at: now()}` to `dismissed_health`, and persists. The "current item" lookup is required because the command receives only `id`; the fingerprint comes from the live state.

## 5. Initial check set

Eleven checks, evaluated in `current_health()`. Suppression: any rule may suppress others by short-circuiting before they're appended.

| `HealthId` | Severity | Trigger | `HealthAction` |
|---|---|---|---|
| `GamelogMissing` | Warn | `status.discovered_logs.is_empty() && config.gamelog_path.is_none()` | `GoToSettings { GamelogPath }` |
| `ApiUrlMissing` | Warn | `config.remote_sync.enabled && config.remote_sync.api_url.is_none()` | `GoToSettings { ApiUrl }` |
| `PairMissing` | Warn | `config.remote_sync.enabled && config.remote_sync.api_url.is_some() && config.remote_sync.access_token.is_none()` | `GoToSettings { PairingCode }` |
| `AuthLost` | Error | `status.account.auth_lost` | `GoToSettings { PairingCode }` |
| `CookieMissing` | Warn | Paired (`api_url` AND `access_token` set) AND `cookie_status.configured == false` AND `hangar.last_attempt_at.is_some()` (i.e., the user has actually engaged hangar features) | `GoToSettings { RsiCookie }` |
| `SyncFailing` | Error | `status.sync.last_error.is_some() && !auth_lost` (suppressed by `AuthLost` to avoid double-reporting the same root cause) | `RetrySync` |
| `HangarSkip` | Warn | `status.hangar.last_skip_reason.is_some() && status.hangar.last_success_at.is_none()` (only when *never* succeeded; a one-off skip after a stream of successes is noise) | `RefreshHangar` |
| `EmailUnverified` | Warn | `status.account.email_verified == Some(false)` | `OpenUrl { format!("{}/verify-email", config.web_origin) }` (item suppressed entirely if `web_origin` is `None`) |
| `GameLogStale` | Warn | `sysinfo` reports a process named `StarCitizen.exe` (Windows) / `StarCitizen` (other) AND `tail.current_path.is_some()` AND `now - tail.last_event_at > 30min` | None (informational) |
| `UpdateAvailable` | Info | A prior auto-update check left a `update_available: Option<UpdateInfo>` on `TrayState`. New field; populated when the existing auto-check fires at startup. | `GoToSettings { Updates }` |
| `DiskFreeLow` | Warn | `sysinfo` reports free bytes on the user-data-dir partition `< 1_073_741_824` (1 GiB) | None (informational) |

**Ordering**: stable sort by `(severity, id)` with Error first. `HealthId` declaration order serves as the tie-breaker — the order in the table above is the order in the enum.

**`CookieMissing` gating rationale**: the cookie is opt-in (the README explicitly says StarStats works without it). Surfacing it on every install would nag users who have no intention of using hangar features. The gate `hangar.last_attempt_at.is_some()` means the item only appears after the hangar worker has at least *tried* — which only happens once both `api_url` and `access_token` exist. So the item is visible exactly to the user who has paired, enabling hangar features by virtue of that pairing, but hasn't pasted a cookie.

## 6. Inline validation probes

Two new commands in `probes.rs`, accessible from `SettingsPane` via a new `InlineCheck` component:

```rust
// crates/starstats-client/src/probes.rs

#[derive(Debug, Clone, Serialize)]
pub struct ApiUrlCheck {
    pub ok: bool,
    pub status: Option<u16>,
    pub server_version: Option<String>,
    pub error: Option<String>,       // friendly category, NOT raw error
}

#[derive(Debug, Clone, Serialize)]
pub struct CookieCheck {
    pub ok: bool,
    pub handle: Option<String>,
    pub error: Option<String>,
}

pub async fn check_api_url(url: String) -> Result<ApiUrlCheck>;
pub async fn check_rsi_cookie(cookie: String) -> Result<CookieCheck>;
```

`check_api_url`:
- Validates `url` is `http(s)://` and parses; if not, returns `ApiUrlCheck { ok: false, error: Some("Invalid URL"), .. }`.
- Issues `GET <url>/healthz` with a 5s timeout via the existing reqwest client. (Confirmed: `crates/starstats-server/src/main.rs:371` exposes `/healthz` via `health::live`.)
- On 200: reads `X-Server-Version` header if present, returns `ok: true`.
- On non-200 or network failure: maps the error via the same friendly-error logic the frontend uses (kept in sync via a small shared mapper).

`check_rsi_cookie`:
- Validates `cookie` is non-empty.
- Reuses the existing RSI HTTP client wiring from `hangar.rs` (cookie injection, user-agent, RSI base URL) to issue one lightweight authenticated request that returns identifiably-authenticated content on success and a non-200 on failure. Specific endpoint to be chosen at implementation time — the existing `PLEDGES_URL` is heavyweight (5MB cap) and is a fallback; a cheaper auth-signalling RSI path is preferred.
- Returns the handle on success (parsed from the response when the chosen endpoint provides it); an error category on failure.
- **Does not persist the cookie.** The user must explicitly hit Save to update `RsiCookieStatus`.

Frontend (`apps/tray-ui/src/components/InlineCheck.tsx`):

```tsx
type CheckState =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'ok'; detail: string }
  | { kind: 'err'; detail: string };
```

A small button + result line, used by `SettingsPane` next to the API URL field and the RSI cookie field. The button is disabled when the input is empty. The result renders inline below the input without mutating the surrounding draft state.

## 7. Friendly error mapping

```ts
// apps/tray-ui/src/lib/friendlyError.ts

export interface FriendlyError {
  title: string;
  body: string;
  hint?: string;
}

export function friendlyError(err: unknown): FriendlyError;
```

Recognised categories (matched against the raw error string from Tauri):

| Match | `title` | `body` | `hint` |
|---|---|---|---|
| `/timeout|timed out/i` | "Timed out" | "The server didn't respond in time." | "Try again, or check your connection." |
| `/connection refused\|dns\|network/i` | "Couldn't connect" | "Couldn't reach the server." | "Check the API URL or your internet." |
| `/401\|403\|unauthori[sz]ed/i` | "Rejected" | "The server rejected this device." | "You may need to re-pair." |
| `/404\|not found/i` | "Endpoint not found" | "The server is up but the endpoint is missing." | "Check the API URL — is it pointing at the right server?" |
| `/5\d\d/` | "Server error" | "The server is having problems." | "Try again in a moment." |
| `/cookie\|rsi.token/i` | "No RSI cookie" | "Paste your RSI cookie to enable hangar sync." | undefined |
| (default) | "Something went wrong" | the raw message, capped at 200 chars (longer values get `…` appended) | "Enable Debug logging in Settings to capture details." |

Every `catch` block in `SettingsPane`/`StatusPane`/`LogsPane` switches from `String(err)` to `friendlyError(err)`. The result is rendered as a short inline message — `title` on its own line, `body` below it, `hint` (if present) in a dimmer style below that.

## 8. HealthCard placement and interaction

- **Position**: top of `StatusPane`, *above* the headline stat strip.
- **Empty state**: `items.length === 0` ⇒ renders `null`. Zero pixels of clutter on a clean install.
- **Replaces**: the existing top-of-Status `Banner` components for `auth_lost` and `email_unverified`. Their state is now carried by `HealthItem`s with the same severities.
- **Kept**: per-card error displays inside Settings (cookie save error, pair error) and inside Hangar (refresh error). The Health roll-up *adds* a top-level aggregate view; inline errors next to the input that produced them remain. Deliberate redundancy — the editing context and the dashboard context have different needs.
- **CTA dispatch** (React side):
  - `GoToSettings { field }` → switch view to `'settings'`, then call `useFieldFocus().focus(field)` which scrolls and focuses via a per-field DOM ref registered by the `SettingsPane`.
  - `RetrySync` → `api.retrySyncNow()`. Result feedback: the existing `useStatusPolling` will reflect success/failure on the next poll (≤15s); no separate toast surface is added. If the call itself throws, the `HealthCard` row replaces its CTA with a one-line `friendlyError().title` inline until the next poll clears it.
  - `RefreshHangar` → `api.refreshHangarNow()`. Same feedback model as above — the Hangar card's existing state machine already renders the in-flight/result transitions.
  - `OpenUrl { url }` → `tauri-plugin-shell.open(url)`. URL was validated http(s) Rust-side at construction time.
- **Dismiss button** (only when `item.dismissible === true`): calls `api.dismissHealth(item.id)`, optimistically removes the row, and refetches `getHealth()` on the next poll.

`useFieldFocus()` is a new tiny shared hook. `SettingsPane` registers refs for each `SettingsField`; `App` exposes `useFieldFocus().focus(field)` which calls the ref's `.scrollIntoView({ behavior: 'smooth', block: 'center' })` and `.focus()`. This decouples the Health card from `SettingsPane`'s internal structure — neither side knows about the other directly.

## 9. New state on `TrayState`

For `UpdateAvailable`, the tray needs to remember the result of the auto-update check beyond Settings-pane local state. Add to `state.rs`:

```rust
pub struct TrayState {
    // ...existing fields...
    pub update_available: Option<UpdateInfo>,  // populated by auto-check at startup
                                               // and by manual "Check for updates"
}

pub struct UpdateInfo {
    pub version: String,
    pub channel: ReleaseChannel,
    pub checked_at: DateTime<Utc>,
}
```

The existing `apply_update` path already restarts the app, so this field doesn't need to be cleared — it's regenerated fresh on the next launch.

## 10. Dependencies

- `sysinfo` — already a workspace dependency (`sysinfo = "0.30", default-features = false` per `Cargo.toml:146`). Used for SC process detection and free-disk-space query. The `starstats-client` crate adds it under `[dependencies]` if not already declared there.
- No new external crates required. The `HealthItem.fingerprint` uses serde-driven canonical JSON, so no hash crate is needed.

## 11. Testing

**Rust (`crates/starstats-client/`):**

- Unit tests in `health.rs` — one per check, covering both `triggers` and `does-not-trigger` shaped fixtures of `TrayState`.
- Unit test for suppression: when `AuthLost` is present, `SyncFailing` is NOT emitted even if `status.sync.last_error.is_some()`.
- Unit test for `HangarSkip`: a skip after a prior success is NOT emitted; a skip with no prior success IS emitted.
- Unit test for fingerprint: same `(id, params)` ⇒ same fingerprint; different params ⇒ different.
- Unit test for dismissal re-emergence: dismiss `SyncFailing { last_error: "502" }`, change to `{ last_error: "401" }`, assert re-emerges.
- Unit test for `CookieMissing` gating: only fires when paired AND a prior hangar attempt exists.
- Integration test: clean state (`remote_sync.enabled = false`, no discovered logs) ⇒ only `GamelogMissing` (the rest are gated on remote-sync being enabled).
- Unit tests for `probes.rs`: not full HTTP tests (those need fixtures), but ensure URL validation rejects non-http(s) and the success/failure mapping to `ApiUrlCheck` is deterministic.

**Frontend (`apps/tray-ui/`):**

- Vitest unit tests for `friendlyError()` against a fixture set of real error strings captured from current production code paths (network errors from `pair_device`, validation errors from `set_rsi_cookie`, etc.).
- Vitest unit tests for `useHealthStrings()` covering every `HealthParams` variant — exhaustive match.
- Vitest component tests for `HealthCard` with fixture item lists: empty, Error-only, Warn+Info mix, dismissable Info, dismissable Warn. Asserts severity ordering, presence/absence of Dismiss button.
- Vitest component test for `InlineCheck` covering idle/running/ok/err states.

**Optional (defer unless cheap):**

- Playwright E2E that boots the tray with `remote_sync.enabled = true` and no `api_url`, asserts `HealthCard` shows `ApiUrlMissing` with "Set up" CTA, clicks CTA, asserts Settings pane is visible and the API URL field has focus.

## 12. Migration and compatibility

- `Config.dismissed_health: Vec<DismissedHealth>` defaults to empty via serde `#[serde(default)]`. Existing configs deserialise unchanged.
- `TrayState.update_available: Option<UpdateInfo>` is in-memory only; no persistence.
- No change to `/v1/events`, manifest format, or on-disk SQLite schema.
- Removed: top-of-Status `<Banner>` JSX for `auth_lost` and `email_unverified` in `StatusPane.tsx`. Their state is now in `HealthItem`s.
- Kept: per-field/per-card error rendering in `SettingsPane.tsx` and the Hangar card in `StatusPane.tsx`.

## 13. Explicitly deferred

These are coherent next steps but out of scope for this spec:

- **First-run modal/wizard.** The inline checklist behaviour of `HealthCard` (it surfaces every setup gap as an item) covers the *informational* need for a new user. A modal-flow walkthrough is a larger UX bet that should wait for evidence of first-install friction.
- **Always-visible header health pip.** A tray-icon coloured dot summarising overall health. Useful but adds platform-specific work (Windows tray-icon updates).
- **Snooze with duration.** Permanent-dismiss-with-fingerprint covers the daily-driver case. Time-bound snooze can be added later by extending `DismissedHealth` with `expires_at: Option<DateTime<Utc>>`.
- **Persistent per-user "ignore" of an entire `HealthId`** (e.g., "I never want to see `DiskFreeLow`"). Possible by adding a separate `silenced_ids: Vec<HealthId>` to `Config`, but a footgun for severe items.
- **Aggregating pipeline-internal health** (backfill stuck, launcher tail idle, parser coverage drop). Pipeline-internal items aren't user-actionable; they stay in the existing Sources card.

## 14. Open questions for implementation phase

- The exact RSI endpoint used by `check_rsi_cookie`. The fallback (`PLEDGES_URL`) works but is heavy; an auth-signalling RSI URL that returns a small body on success would be preferable. Implementation pass should evaluate candidates (e.g., `https://robertsspaceindustries.com/api/account/me` or similar lightweight RESTish endpoints) and lock the choice.
- Process name for SC on non-Windows. Star Citizen is Windows-only today, so the cross-platform sysinfo check is defensive future-proofing. Implementation may choose to skip the `GameLogStale` check on non-Windows entirely rather than attempt a futile match.

---

*Implementation plan to follow via `writing-plans` skill, sequencing per dependency: shared types & enums → `health.rs` derivation → tests → `probes.rs` → `commands.rs` wrappers → `useHealth` + `friendlyError` → `HealthCard` + `InlineCheck` → SettingsPane integration → remove obsolete Banners → E2E.*
