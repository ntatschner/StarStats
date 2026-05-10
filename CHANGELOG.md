# Changelog

All notable changes to StarStats will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0, semver applies in spirit only — the wire format, schema, and
event coverage are still evolving and may break on minor releases.

Tag-suffix → release-channel mapping (see `release-manifests/`):

- `vX.Y.Z-alpha[.N]` → `alpha.json`
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
