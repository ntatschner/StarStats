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

[Unreleased]: https://github.com/ntatschner/StarStats/compare/v0.3.2-alpha...HEAD
[0.3.2-alpha]: https://github.com/ntatschner/StarStats/compare/v0.3.1-alpha...v0.3.2-alpha
[0.3.1-alpha]: https://github.com/ntatschner/StarStats/compare/v0.3.0-alpha...v0.3.1-alpha
[0.3.0-alpha]: https://github.com/ntatschner/StarStats/compare/v0.2.0-alpha...v0.3.0-alpha
[0.2.0-alpha]: https://github.com/ntatschner/StarStats/compare/v0.1.0-alpha...v0.2.0-alpha
[0.1.0-alpha]: https://github.com/ntatschner/StarStats/releases/tag/v0.1.0-alpha
