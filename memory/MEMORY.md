# Project Memory

## Active Goals
- Diagnose the live `/sharing` failure — fix shipped 2026-05-17 (`8fdb566`) switches to `Promise.allSettled` + per-call logging. Next page load names the failing endpoint in server logs as `sharing data fetch rejected call=<name> status=<code>`. User to share log line so we can target root cause.
- Decide what to do about C2 (ss-eyebrow sweep, 45 files) once the audit doc is available.
- Triage 2 moderate Dependabot alerts on default branch.

## Current Status
- **Workspace version:** 0.0.7-beta (bumped `bff9107`, 2026-05-17).
- **Migration tip:** 0028 (users.shares_paused_until).
- **Server tests:** 299/299 passing (added 2 pause-gate tests in Wave 4).
- **Active branch:** main. Wave work merges directly; `feat/tray-followups` already merged via PR #22.
- **Audit v2 waves landed (most recent first):**
  - `5b4a3ab` — Audit v2.1 §C auto-pause closure: migration 0028 + UserStore methods + add_share gate + cluster-fire stamps column
  - `7441f22` — events v2: server synthesises EventMetadata for legacy v1 clients
  - `7759935` — Audit v2.1 §C: per-user sharing context admin sub-tab
  - `13084cf` — Audit v2.1 §B1: Preview as @handle (simulated)
  - `0a33160` — Audit v2.1 Wave A: scope-preset chips + bulk ops
  - `8fdb566` — fix(sharing): tolerate partial-failure on the 4-call dashboard load
  - `0e91cbf` — Wave C tail: h1 type plateau (28px baseline) + HangarCard reframe
  - `989fbac` — Wave B2: share-reports moderation queue (store + 3 endpoints + reporter affordance + admin queue page)

## Technical Environment
- Windows 11 + PowerShell + WSL. Project at `D:\git\RSIStarCitizenTools\StarStats`.
- Rust 1.88 MSRV (from `Cargo.toml [workspace.package]`). aws-sdk-s3 pinned to `=1.110.0` because 1.111+ requires rustc 1.91.
- pnpm + Node >= 20.
- CI: rustfmt --check + clippy -D warnings + cargo test + pnpm typecheck + pnpm lint + Playwright. All must pass.

## Decisions Log
- 2026-05-16 — Audit v2 polish wave executed in 5 sub-waves (A, B1, B2, C1-C3) with one commit per wave. Agent teams used inside each wave where files were file-isolated; main session finished waves when agent budget ran out.
- 2026-05-16 — HangarCard refresh DECISION: option (b) reframe as "Updated via tray · open Devices →". Server holds zero RSI cookies by design; option (a) cookie-on-server was rejected as it'd break the OS-keychain trust boundary.
- 2026-05-16 — C1 type plateau DECISION: option (a)-lite — set `main h1 { font-size: 28px }` baseline in globals.css + drop the 3 inline `fontSize: 32` overrides on admin/sharing and admin/sharing/audit. /sharing keeps its inline 32 as a top-level user surface.
- 2026-05-16 — C2 eyebrow sweep DEFERRED — 45 files is taste-heavy work that needs the audit doc's "which to keep" rule. Speculative bulk-delete rejected.
- 2026-05-17 — `/sharing` resilience: switched 4-call dashboard load from `Promise.all` -> `Promise.allSettled` with per-rejection logging. Rationale: monolithic catch swallowed which endpoint was failing; the new shape both surfaces partial data AND names the failure in logs.

## Known Constraints
- Server cannot proxy RSI authenticated reads (no cookie). Hangar + RSI profile refresh originate from the tray only.
- SpiceDB outages map to 503 -> `spicedb_unavailable` banner, not generic failure.
- audit_log rate-limit reads are capped at the 500-row page size — fine for current homelab volume; add a dedicated COUNT(*) if it grows.
- Workspace clippy `-D warnings` — every warning fails CI. Allow at workspace level only with documented rationale.
- aws-sdk-s3 + aws-credential-types + aws-smithy-runtime-api are pinned to 1.88-MSRV-compatible versions; bumping requires rustc toolchain bump in lockstep.

## Glossary
- **wave** → a grouped batch of audit-v2 changes shipped as one commit.
- **share scope** → JSONB clamp on `share_metadata.scope` controlling what a recipient can see (`kind` in {full, timeline, aggregates, tabs}; optional allow/deny event-type lists, window_days).
- **share_reports** → moderation queue table (migration 0027) backing `/v1/share/report` (user file) and `/v1/admin/sharing/reports*` (moderator triage).
- **PostgresShareReportStore / MemoryShareReportStore** → prod + test impls of the `ShareReportStore` trait.
- **AdminSharingOverview / ScopeHistogram** → DTOs powering the `/admin/sharing` headline cards; replaced an earlier audit-log-window proxy that silently undercounted.
- **RequireModerator** → axum extractor in `admin_routes.rs` that gates routes on moderator role (admins inherit).
- **device_id** → audit_log payload field on `ingest.batch_processed` rows; partial index `audit_log_ingest_device_idx` powers per-device activity slicing.
