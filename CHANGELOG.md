# Changelog

All notable changes to StarStats will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0, semver applies in spirit only — the wire format, schema, and
event coverage are still evolving and may break on minor releases.

Tag-suffix → release-channel mapping (see `release-manifests/`):

- `vX.Y.Z-alpha[.N]` → `alpha.json`
- `vX.Y.Z-beta[.N]`  → `beta.json`
- `vX.Y.Z-rc[.N]`    → `rc.json`
- `vX.Y.Z`           → `live.json`

## [Unreleased]

### Added

- (nothing yet)

### Changed

- (nothing yet)

### Fixed

- (nothing yet)

### Security

- (nothing yet)

## [0.0.7-beta]

### Added

- **Audit v2.1 §B1 — "Preview as @handle".** Owners can simulate a
  recipient's view of their own data through a candidate scope before
  granting the share. New `/v1/me/preview-share/{summary,timeline}`
  endpoints render the owner's own data through the proposed scope
  clamp (no SpiceDB check, no audit row — it's a simulation). New
  `/sharing/preview` server-rendered page with a sticky simulation
  banner and empty states for scope-excluded surfaces.
- **Audit v2.1 §C — per-user sharing context.** New admin sub-tab
  surfaces a user's outbound shares, inbound shares, reports filed,
  and reports filed against them, all in one place. Backed by
  `/v1/admin/sharing/by-user/:handle`.
- **Audit v2.1 §C — abuse-signal detection.** `add_share` now checks
  for a rapid-grant cluster (≥15 grants/24h → 429
  `rate_limited_rapid_grant` + `share.signal_rapid_grant` audit row).
  `report_share` checks for a cross-report cluster (≥3 reports
  against one owner/72h → `share.signal_cluster_pause` audit row).
- **Audit v2.1 §C — auto-pause closure.** When the cross-report
  cluster threshold fires, the owner's `users.shares_paused_until` is
  stamped with a 24h ban. `add_share` gates on it up-front: paused
  owners get 403 `shares_paused` before any recipient lookup or
  SpiceDB write. Migration 0028 lands the column (additive, NULL
  default, partial index).
- **Audit v2.1 Wave A — sharing presets + bulk ops.** Three scope-
  preset chips on the share editor (Friend / Org / Public) and a
  bulk-ops row above the outbound list (revoke-expired,
  reset-scope-on-all-active).
- **Events v2 metadata envelope.** `EventEnvelope` now carries an
  optional `EventMetadata` field (`source`, `entity_refs`,
  `provenance`, `event_type_key`, `group_key`). v1 batches still
  accepted — server synthesises observed metadata for legacy clients
  via `stamp()`. `IngestBatch::CURRENT_SCHEMA_VERSION` bumped to 2.
- **Type-plateau pass (audit v2 §03).** `main h1` baseline set to
  28px in globals.css; top-level pages opt to 32px inline, deep
  detail pages can opt to 24px. HangarCard refresh affordance
  reframed as "Updated via tray · open Devices →" — honest framing
  for the server-holds-zero-credentials architecture.
- **Project automation seed.** `.claude/` tree with two hooks
  (rustfmt-on-edit, protect-migrations), two skills (regen-openapi,
  new-store), and two reviewer agents (migration-safety-reviewer,
  api-contract-reviewer). Project memory tree (CLAUDE.md +
  memory/MEMORY.md).

### Changed

- **Sharing dashboard load is partial-failure tolerant.** The
  `/sharing` page now uses `Promise.allSettled` across its four
  underlying API calls, logs each rejection with `call=<label>`, and
  surfaces SpiceDB-unavailable as a banner rather than blanking the
  whole render.

### Fixed

- **`/sharing` no longer blanks on a single endpoint hiccup.** The
  previous `Promise.all` race meant any one of `getVisibility`,
  `listShares`, `listSharedWithMe`, or `listOrgs` returning a
  non-2xx wiped the entire page with "Couldn't load your sharing
  state."

## [0.0.5-beta] — 2026-05-16

### Added

- **Tray Health surface.** New "Health" section at the top of Status
  aggregates every actionable setup/lifetime problem into a single
  list — Game.log missing, API URL missing, pairing missing,
  auth-lost, sync failing, hangar skipped, RSI cookie missing while
  paired, email unverified, Game.log stale while SC is running,
  update available, low disk space. Each item gets a per-row CTA
  (Set up / Retry sync / Refresh now / Open). Info- and Warn-severity
  items are dismissible; dismissals re-emerge when the underlying
  params change (fingerprint over the payload, not the id).
- **Inline configuration probes.** New "Test connection" and "Test
  cookie" buttons next to the API URL and RSI session-cookie inputs
  in Settings. Both perform a single 5-second HTTPS round-trip and
  render the result inline — no need to save and wait for the next
  sync cycle to learn if the value works.
- **Friendly error messages.** Tauri command failures in
  SettingsPane and the hangar refresh card are now categorised
  (timeout / connection refused / 401-403 / 404 / 5xx / no cookie /
  unknown) instead of rendered as raw error strings.
- **Vitest test runner** for `apps/tray-ui/` — 31 frontend tests
  cover the new pure modules and components.

### Changed

- The top-of-Status `auth_lost` and `email_unverified` banners are
  replaced by `HealthItem`s in the new Health card with the same
  severities. Per-card inline error displays (cookie save error,
  pair error, hangar refresh error) are retained — the Health card
  aggregates without removing the editing-context affordances.
- HealthCard CTAs that navigate to Settings now focus the relevant
  field via a new `useFieldFocus` registry (one provider at App
  root, ref callbacks on each registered field).

## [0.0.4-beta] — 2026-05-16

Tray design-language polish. The tray UI's tokens and primitives
already mirrored the design system, but the user-visible identity
layer — fonts, theme switching, entrance motion — wasn't wired up.
This release closes those gaps from the design handoff audit.

### Added

- **Tray:** Geist + Geist Mono bundled via `@fontsource-variable`
  so the `--font-sans` / `--font-mono` tokens resolve to the design
  system's signature typeface. Bundled (not CDN) because the Tauri
  CSP is `default-src 'self'`.
- **Tray:** `Theme` enum (Stanton / Pyro / Terra / Nyx) added to
  `Config` with serde defaults and backward-compat parsing (old
  config.toml files without the field load as Stanton).
- **Tray:** Settings → Appearance card with four-swatch picker.
  Eager preview flips `data-theme` on click; Save persists.
- **Tray:** `.ss-screen-enter` wrapper on the active pane fires
  the design system's card-stagger motion on every tab switch;
  `TrayCard` adopts `className="ss-card"` so the mount/hover
  animations engage.

### Changed

- **Tray:** Unverified-Comm-Link banner copy tightened to match
  the audit's in-universe voice ("Comm-Link unverified — claim
  it before someone else can").

## [0.0.3-beta] — 2026-05-12

Tray-UI half of the metrics-display redesign. v0.0.2-beta shipped the
new charts on the web app only; this release brings a tray-native
equivalent so the desktop window also benefits.

### Added

- **Tray:** `EventSparkline` component — 48-hour rolling sparkline
  of events/hour, rendered inline-SVG against the `--accent` token
  (no chart library — keeps the Tauri bundle slim). Lands in
  `StatusPane.tsx` above the existing "Top event types" card under
  the heading "Recent activity · 48h". Consumes the timeline the
  tray already fetches; no new IPC.

### CI

- Workflow split: container/config images now live in a sibling
  `release-images.yml` workflow so a registry-side outage no longer
  marks the tray release as failed. Both workflows trigger on the
  same `v*` tag and can be re-run independently via
  `workflow_dispatch`.
- `Release tray` now detects already-published GitHub Releases and
  skips the asset-upload + draft-promotion steps, so re-runs on
  already-shipped tags no longer fail at "Cannot delete asset from
  an immutable release". The channel-manifest commit step stays
  unguarded so it can still recover a missing manifest.
- Channel-manifest commit step now uses `git add` + `git diff
  --cached --quiet` instead of `git diff --quiet` against an
  untracked path — fixes the bug that silently skipped the
  first-ever `release-manifests/beta.json` publish in v0.0.1-beta
  and v0.0.2-beta.

## [0.0.2-beta] — 2026-05-12

Metrics-display redesign, first wave. Replaces the hand-rolled 30-day
heatmap on the dashboard with a GitHub-style 53-week heatmap, and
rewires the metrics page's Overview tab with a donut+barlist Type
breakdown alongside the heatmap. Foundational shell + lib helpers
land so subsequent waves can layer in without rebuilding the chart
contract.

### Added

- **Web:** `YearHeatmap` component — 53-week GitHub-style activity
  heatmap, inline SVG, renders against the `--grid-*` token ladder
  for theme reactivity. Shown on `/dashboard` and `/metrics` Overview.
- **Web:** `TypeBreakdown` component — recharts donut + ranked-bar
  combo replacing the manual `<div>` bars previously on the metrics
  Overview tab.
- **Web:** `SparklinePill` component — small stat tile with inline
  sparkline. Foundation for upcoming dashboard pill upgrades.
- **Web:** `MetricCard` + `ChartCard` shells. Required-props pattern
  (`flagKey`, `telemetryKey`, `empty`, `error`, `srTable`) enforces
  the cross-cutting checklist (feature-flag gate, telemetry hook,
  empty/error states, screen-reader fallback) at the TypeScript
  level — cards that skip any of these fail `tsc`.
- **Web:** Typed feature-flag registry (`lib/feature-flags.ts`) for
  the metrics surfaces. All flags default on for v0.0.2-beta; the
  `metrics.now_strip` flag stays off (cut per the impl plan).
- **Web:** Frontend telemetry helper (`lib/metrics-telemetry.ts`).
  Opt-in (off by default via `localStorage["starstats.telemetry"]`).
  Server endpoint to receive POSTs is a follow-up.
- **Web:** Recharts theme bridge (`lib/recharts-theme.ts`) — reads
  `ss-*` CSS-var hex values from `:root` and re-renders on
  `data-theme` mutations so chart colours swap with the active theme.
- **Server:** Migration `0021_share_scopes.sql` — adds a per-user
  `share_scopes` JSONB column with conservative defaults (own data
  only for everything except summary, which defaults to friend).
  Future-proofs the planned cross-user aggregate endpoints; no code
  consumes the column yet.
- **Dep:** `recharts ^3.8.1` in `apps/web`.

### Changed

- **Server:** Bumped `TIMELINE_DAYS_MAX` from 90 to 366 in
  `validation.rs` so `YearHeatmap` callers can request a 365-day
  window without tripping the validator. Existing
  `timeline_rejects_days_above_max` test updated accordingly.
- **Web:** `/dashboard` and `/metrics` now request a 365-day timeline
  (was 30). DayHeatmap still renders cleanly with the wider window.
- **Versions:** Workspace `0.0.1-beta → 0.0.2-beta`,
  `tauri.conf.json` `0.0.1 → 0.0.2`.

### Documentation

- Added `docs/DESIGN-METRICS-PLAN.md` (strategic plan) and
  `docs/DESIGN-METRICS-IMPLEMENTATION-PLAN.md` (execution plan,
  reflecting two rounds of independent review findings).

## [0.0.1-beta] — 2026-05-11

Fresh start on the `beta` channel after the alpha history scrub.
Versions reset from `0.3.12-alpha` to `0.0.1-beta`; the prior alpha
tags and releases were removed from the public repository.

### Added

- **Client:** New `Beta` variant on `ReleaseChannel` (Cargo + tray-UI),
  with a matching `beta.json` channel manifest produced by the release
  workflow. The Settings → Updates dropdown now offers Beta alongside
  Alpha / RC / Live.
- **Release workflow:** Added the `v*.*.*-beta[.N]` case to the
  channel-pattern matcher so beta tags publish to
  `release-manifests/beta.json` on `main`.

### Changed

- **Client:** `ReleaseChannel::default()` is now derived from
  `CARGO_PKG_VERSION` at compile time rather than being a hard-coded
  `Alpha`. A build tagged `vX.Y.Z-beta` defaults fresh installs to the
  Beta channel; the future first stable build will default to Live.
  Persisted user overrides in `config.toml` still win over the default.
- **Client:** Tauri bootstrap updater endpoint flipped from `alpha.json`
  to `beta.json` for this build (only relevant on first launch before
  the channel-aware override fires).
- **Versions:** Workspace `0.3.12-alpha` → `0.0.1-beta`,
  `tauri.conf.json` `0.3.12` → `0.0.1`.

## [0.3.12-alpha] — 2026-05-11

### Added

- **Server:** DB-backed SMTP config with KEK-encrypted password and
  hot-reload. New migration `0020_smtp_config.sql` (singleton row
  enforced by `CHECK (id = 1)`, password split into BYTEA
  `ciphertext` + `nonce` columns with a paired-NULL check). New
  `smtp_config_store` module + Postgres impl that encrypts on write /
  decrypts on read via the existing TOTP KEK envelope. The `Mailer`
  trait gains `send_test_email`, and a new `SwappableMailer` wraps
  the active transport in `Arc<RwLock<Arc<dyn Mailer>>>` so the
  admin save flow can replace it without restarting the server. Boot
  precedence is DB(enabled=true) > env > `NoopMailer`.
- **Server:** Three new admin endpoints — `GET/PUT /v1/admin/smtp`
  (read/write the config with password redaction + `password_set`
  bool) and `POST /v1/admin/smtp/test` (sends a diagnostic email to
  the calling admin's verified address; 400 if unverified, 502 on
  SMTP failure). All gated by `RequireAdmin`. `PUT` validates input,
  persists, then swaps the live mailer.
- **Web:** New `/admin/smtp` page with hot-reloading config form.
  Server actions thread the bearer through the existing
  `lib/api`-is-server-only invariant; client form holds controlled
  state with a tri-state password (null = keep, "" = clear, value =
  set) mirroring the server contract. Save / Send test / Reload
  buttons gated by `useTransition` for clean pending UI. New tab in
  `AdminNav` between Submissions and Audit log.
- **Server:** `SpicedbClient::write_owner(handle)` issues TOUCH on
  `stats_record:<handle>#owner@user:<handle>`. The signup handler
  calls it best-effort after `users.create()` so the
  `stats_record.view` permission is non-empty for every new account
  — unblocks any future reinstatement of the SpiceDB self-view gate
  in `query::summary`.

### Changed

- **Tray:** Sync worker now respawns on config save and on device
  pairing. `AppState` holds the running `JoinHandle`; new
  `sync::respawn` aborts the old worker, reloads the persisted
  config, and spawns a fresh one. `save_config` and `redeem_pair`
  call it after `config::save`, so toggling Settings or pairing a
  new device picks up immediately — no more "save settings →
  restart tray" contract. Idempotent: disabling sync swaps the
  handle to `None`.
- **Web:** `QuantumWarp` background re-aims per route. The
  prototype's `warpAngle = angleFor(screen)` wiring was never
  ported to the production Next.js code, so the canvas was stuck
  at the default 180° regardless of which page was active. New
  `QuantumWarpBackground` client wrapper reads `usePathname()` and
  maps to an angle via a static `FIXED` table (mirrors the
  prototype's intuition; deterministic hash fallback for unmapped
  paths). Tween rate bumped 0.04 → 0.08 (~12 frames / ~200ms) so
  the direction change is visually obvious within the brief's
  500ms target.

### Fixed

- **Server:** Drop the `require_user_token` gate from hangar /
  RSI-profile / RSI-org routes. Pairing only mints device JWTs, so
  the gate was locking the tray out of exactly the endpoints it
  was built to feed (e.g. `hangar push failed: 403 Forbidden`).
  Identity is still enforced by `AuthenticatedUser`; the gate
  added no security on top.
- **Web:** Logout no longer sends the user to
  `https://0.0.0.0:3000/`. `route.ts` used to build the redirect
  URL from `req.url`, which inside the container is
  `http://0.0.0.0:3000/auth/logout`; the reverse proxy upgraded
  the scheme to https and the host was wrong. Replaced with a
  relative `Location: /` so the browser resolves against the URL
  it actually typed.
- **Server:** `cargo fmt` drift in `starstats-client/{commands.rs,
  storage.rs}` cleared so subsequent pushes pass CI's
  `cargo fmt --check`.

## [0.3.11-alpha] — 2026-05-10

### Added

- **Tray:** Re-parse now retroactively detects bursts over already-
  stored events. New Phase 3 walks each `log_source` in
  `source_offset` order, runs `detect_bursts` over the
  structural-parsed view, inserts one `BurstSummary` per hit, and
  hard-deletes the member rows. Surfaces `bursts_collapsed` and
  `members_suppressed` in `ReparseStats`; the *Re-parse* status line
  reports `…collapsed N bursts (suppressed M spam rows)…` when the
  pass fires. Idempotency key reuses the live-tail format
  (`UUIDv5(log_source : anchor_offset : "{raw_line}|burst:{rule_id}:{size}")`)
  so a session already collapsed at live-ingest time stays a
  strict no-op, and re-running Phase 3 over post-collapse history
  finds nothing to do.
- **Storage:** Three new lean helpers on `Storage` —
  `distinct_log_sources()`, `events_for_burst_scan(log_source)`
  (returns `(id, raw, source_offset, type)` ordered by source
  offset), and `delete_event_by_id(id)`. The first two scope retro-
  burst's working set to one channel at a time so spam-clusters
  spanning channel boundaries can't accidentally fuse.

## [0.3.10-alpha] — 2026-05-10

### Added

- **Core:** New `templates` module providing two deterministic
  group-recognition primitives — `EventTemplate` for fixed-sequence
  ritual matching with drift detection, and `BurstRule` for
  variable-cardinality clustering with anchor + member + slack
  budget. Both serialise/deserialise as JSON so future remote
  delivery via `/v1/parser-definitions` is a drop-in.
- **Core:** New `GameEvent::BurstSummary` variant carrying
  `rule_id`, `size`, `end_timestamp`, and a truncated
  `anchor_body_sample`. Validated server-side (non-empty rule id,
  size > 0, ISO-8601 end timestamp).
- **Tray:** Four built-in `BurstRule` definitions in
  `crates/starstats-client/src/burst_rules.rs` —
  `loadout_restore_burst`, `terrain_load_burst`,
  `hud_notification_burst`, `vehicle_stowed_burst` — collapse the
  four spammiest event clusters observed in real Game.log captures.
- **Tray:** `gamelog::process_buffer` ingests in drain-bounded
  batches; `detect_bursts` runs over the structurally-parsed subset,
  emits one `BurstSummary` per hit, and suppresses member events
  from being inserted at all. Idempotency key includes
  `(anchor_offset, rule_id, size)` so retries after a tray crash
  dedupe cleanly.
- **Web:** Timeline renders `burst_summary` events with friendly
  per-rule labels ("Loadout restored", "Terrain loaded",
  "Notifications", "Vehicles stowed"); future remote-served rules
  fall back to a generic "Burst" label.

## [0.3.9-alpha] — 2026-05-09

### Changed

- **Tray:** "Discovered logs" status card collapses the per-file
  list into a count + per-kind chip breakdown. Removes 4–10 rows of
  per-path detail from the main status surface; the tray still
  reads every discovered log, the UI just summarises.

## [0.3.8-alpha] — 2026-05-09

### Fixed

- **RSI parsers:** All three HTML scrapers (orgs, public profile,
  tray hangar) silently produced empty results because their CSS
  selectors were authored against synthetic test fixtures rather
  than RSI's real markup. Rewritten against verified live DOM
  captured 2026-05-09: orgs key off `box-content org main|affiliation`
  containers with labelled SID/rank entries; profile widens scope
  from `.profile .entry` to `.profile-content .entry` (Enlisted /
  Location / Bio live in a sibling `.left-col` outside `.profile`);
  pledges read hidden-input `value=` attributes (not text content).

### Changed

- **CI:** clippy + test gate widened from `core+server` to
  `core+server+client`. Adds `pnpm install` + tray-ui Vite build +
  Linux Tauri system deps (libwebkit2gtk-4.1-dev, libgtk-3-dev,
  etc.) so the Tauri proc-macro can compile against a populated
  `apps/tray-ui/dist`. Pre-existing client clippy warnings resolved
  (`while_let_loop` in `read_capped_text`, `manual_clamp` in
  `clamp_timeline_limit`).

## [0.3.7-alpha] — 2026-05-09

### Added

- **Server:** Admin foundation. New `staff_roles` table with
  soft-delete revocation (`partial unique index … WHERE
  revoked_at IS NULL`); `RequireModerator` / `RequireAdmin` axum
  extractors; bootstrap-from-env helper
  (`STARSTATS_BOOTSTRAP_ADMIN_HANDLES`); admin submission
  moderation routes (accept / reject / dismiss-flag / queue) with
  idempotent state transitions and audit-log writes.
- **Web:** `/admin` shell + `/admin/submissions` moderation queue
  with status filters, paginated list, and per-row server actions.
  Left-rail conditionally renders "Staff › Admin" when the session
  carries staff roles.
- **Web:** RSI-orgs surface — `getMyRsiOrgs` / `getPublicRsiOrgs` /
  `refreshRsiOrgs` API helpers; `OrgsCard` component shared between
  dashboard and `/u/[handle]`; main org sorted first.
- **Web:** Public/friend timeline heatmap rendered on
  `/u/[handle]` mirroring the dashboard treatment.
- **Web:** Hangar parity — `getMyHangar` (404 → null) + new
  `HangarCard` component on dashboard and settings.

### Changed

- **Server:** Renamed `query::ListResponse` → `query::EventsListResponse`
  to eliminate an OpenAPI schema collision with
  `submission_routes::ListResponse` (utoipa keys component schemas
  by Rust type name; the collision silently dropped one of the two
  from the spec).
- **Web:** Replaced hand-rolled `CommerceTransaction` and
  `UserPreferences` types with intersections over the generated
  `apiSchema` types; the narrow `kind` / `status` unions are
  preserved via `Omit<…> &` overlay.

### Fixed

- **Tray:** `RedeemResponse.device_id` is now captured into
  storage instead of being dropped (held under `#[allow(dead_code)]`
  until the self-revoke UI lands).

## [0.3.6-alpha] — 2026-05-08

### Added

- **Tray:** Hangar card surfaces affirmative RSI-fetch status
  (last successful refresh + ship count) instead of a silent empty
  pane when the cookie path is healthy.

## [0.3.5-alpha] — 2026-05-08

### Fixed

- **Tray:** `set_rsi_cookie` IPC contract — frontend was sending a
  flat `{cookie}` payload while the Tauri command expected a
  wrapped struct; dropped the wrapper so the IPC matches.

## [0.3.4-alpha] — 2026-05-08

### Fixed

- **Tray:** Header version now reads from the real Cargo workspace
  `[workspace.package].version` instead of a stale hard-coded
  constant.

## [0.3.3-alpha] — 2026-05-08

### Added

- **Tray:** *Re-ingest* button under the Events tab — replays the
  raw rotated `Game-*.log` files through the current parser, so
  newly-added event types backfill historical sessions without
  requiring the user to keep the original `Game.log` around.
- **Repo:** Project front-door (CONTRIBUTING, SECURITY,
  CODE_OF_CONDUCT, EAC-SAFETY, NOTICE) + starstats.app domain
  wiring across README and docs.

### Fixed

- **Storage:** `for_each_event` releases the per-batch SQLite
  connection lock between batches so the writer can make progress
  on large local stores during a Re-parse.

## [0.3.2-alpha] — 2026-05-08

### Fixed

- **Tray:** Re-parse no longer deadlocks on large local stores. The
  per-batch SQLite connection lock is now released between batches in
  `for_each_event`, letting the writer make progress while the
  re-classify pass walks history.

## [0.3.1-alpha] — 2026-05-08

### Added

- **Parser:** Modern `PlayerDeath` and `PlayerIncapacitated` event
  variants matched against the corpse-cleanup burst that replaces
  CIG's old `<Actor Death>` line in 4.x+ Star Citizen builds. The
  legacy `ActorDeath` variant is retained for older logs.
- **Parser:** Zone enrichment for the new death events — quantum-target
  and `Seed Solar System` context are folded into the surfaced event
  so the tray can show *where* a death happened, not just that one
  occurred.

### Changed

- Updated `release-manifests/alpha.json` to point the alpha channel
  at v0.3.1-alpha.

## [0.3.0-alpha] — 2026-05-07

### Added

- **Updater:** Channel selector in *Settings* with three channels —
  Alpha, RC, Live — backed by per-channel updater manifests at
  `release-manifests/{alpha,rc,live}.json` on `main`. The release
  workflow now picks the destination manifest from the tag's
  pre-release suffix (`-alpha` / `-rc` / bare semver).
- **Tray:** *Re-parse* button in the *Events* tab. Re-classifies
  every event already in the local store against the current parser
  without needing to replay `Game.log` from disk — useful after the
  parser learns a new variant.
- **Tray:** Workspace version is now surfaced in *Settings* so the
  installed build matches the corresponding release tag at a glance.

### Fixed

- **Updater:** Per-channel manifest fix — Tauri's updater previously
  only handled `releases/latest`, which 404s for pre-releases. The
  in-app updater now polls the explicit per-channel JSON via
  `raw.githubusercontent.com`, giving every channel a stable URL.

### Changed

- Workspace version bumped 0.2.0-alpha → 0.3.0-alpha.

## [0.2.0-alpha] — 2026-05-04

### Added

- **Parser:** Dynamic parser definitions decoupled from the Rust
  build — new `Game.log` token shapes can be added through the
  versioned definition table without recompiling the tray.
- **API:** `GET /v1/commerce/recent` endpoint surfacing paired
  buy/sell transactions for the authenticated user.
- **Server / parser:** Transaction pairing — `ShopBuyRequest` /
  `ShopFlowResponse` and `CommodityBuyRequest` / `CommoditySellRequest`
  pairs are now matched into a single completed-order record with
  resolved price, quantity, and location.
- **Tray:** *Commerce* tab surfacing paired transactions, totals, and
  per-location breakdowns.
- **Installer:** WiX upgrade metadata so MSI installs from prior
  alphas now upgrade in place rather than installing side-by-side.

### Changed

- Workspace version bumped 0.1.0-alpha → 0.2.0-alpha.
- Release pipeline split into a two-step draft + publish to satisfy
  GitHub's immutable-release policy when the same tag is retried.
- Release pipeline now accepts pre-release tag suffixes against
  numeric MSI bundle versions (the MSI version field is numeric-only;
  the tag carries the `-alpha` / `-rc` annotation separately).

### Security

- Bumped `tauri` 2.11.0 → 2.11.1 to pick up the fix for
  [GHSA-7gmj-67g7-phm9](https://github.com/advisories/GHSA-7gmj-67g7-phm9).

## [0.1.0-alpha] — 2026-05-03

### Added

- Initial public release.
- Tauri tray client with `Game.log` tail, local SQLite store, and
  signed updater bundles for Windows (NSIS + WiX MSI) and Linux
  (AppImage + .deb).
- StarStats API server (Axum + sqlx + Postgres) with self-hosted
  JWT auth, device pairing, ingest, query endpoints, OIDC discovery,
  audit log, and Prometheus `/metrics`.
- Next.js 15 web companion with sign-up / sign-in, email
  verification, dashboard, and device management.
- Initial parser coverage: `ProcessInit`, `LegacyLogin`, `JoinPu`,
  `ChangeServer`, `SeedSolarSystem`, `ResolveSpawn`, `ActorDeath`
  (legacy), `VehicleDestruction`, `HudNotification`,
  `LocationInventoryRequested`, `PlanetTerrainLoad`,
  `QuantumTargetSelected`, `AttachmentReceived`, `VehicleStowed`,
  `GameCrash`, `LauncherActivity`, `RemoteMatch`,
  `MissionStart` / `MissionEnd`, `SessionEnd`.

[Unreleased]: https://github.com/ntatschner/StarStats/compare/v0.3.11-alpha...HEAD
[0.3.11-alpha]: https://github.com/ntatschner/StarStats/compare/v0.3.10-alpha...v0.3.11-alpha
[0.3.2-alpha]: https://github.com/ntatschner/StarStats/compare/v0.3.1-alpha...v0.3.2-alpha
[0.3.1-alpha]: https://github.com/ntatschner/StarStats/compare/v0.3.0-alpha...v0.3.1-alpha
[0.3.0-alpha]: https://github.com/ntatschner/StarStats/compare/v0.2.0-alpha...v0.3.0-alpha
[0.2.0-alpha]: https://github.com/ntatschner/StarStats/compare/v0.1.0-alpha...v0.2.0-alpha
[0.1.0-alpha]: https://github.com/ntatschner/StarStats/releases/tag/v0.1.0-alpha
