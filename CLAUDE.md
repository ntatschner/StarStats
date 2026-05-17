# StarStats

Self-hosted personal Star Citizen metrics — Rust API + Next.js web + Tauri tray.

## Mission / Current Focus
- Closing out the audit-v2 design pass. Backend + admin surfaces done; ss-eyebrow sweep deferred until the audit doc is available to scope it.

## Tech Stack
- **Monorepo:** pnpm workspaces (apps/web, apps/tray-ui, packages/*) + Cargo workspace (crates/starstats-{core,server,client}).
- **API:** Rust + axum + sqlx (Postgres) + JWT auth (claimed handle via `preferred_username`, device_id optional).
- **Authz:** SpiceDB (Zanzibar/ReBAC). 503 from SpiceDB-dependent endpoints surfaces as `spicedb_unavailable` banner.
- **Web:** Next.js 15 App Router (server components, server actions, useFormStatus). Auth via `getSession()` returning `{token, claimedHandle}`.
- **Desktop:** Tauri 2 (React+TS frontend, Rust backend). Tray owns the RSI session cookie — held in the OS keychain.
- **Spec:** utoipa derives OpenAPI; regenerate TS client via `pnpm --filter api-client-ts run generate` (script spawns `starstats-server-openapi` bin).
- **Audit:** `audit_log` is hash-chained, append-only. Share actions: `share.created | revoked | viewed | reported | report_resolved | visibility_changed`.

## Architecture Invariants
- **Server holds ZERO RSI credentials.** Only the tray scrapes `https://robertsspaceindustries.com/account/pledges` using the user's RSI cookie. Any "Refresh" affordance for hangar-style data MUST point at the tray (`/devices`), not promise a server-side fetch. HangarCard documents this; do not break the pattern.
- **Migrations are additive only.** `IF NOT EXISTS`, NULL defaults, no DROPs, no NOT NULL on populated columns without defaults. Backward-compat parsing in Rust uses `#[serde(default)]`.
- **Audit emission is best-effort.** Every audit `.append()` call wraps in `if let Err(e) = ... { tracing::warn!(...) }` so an audit hiccup never poisons the response.
- **Multi-endpoint web dashboards use `Promise.allSettled`**, not `Promise.all`. A single endpoint hiccup must not wipe the whole render. Log each rejection individually with `call=<label> status=<code>` so the failing endpoint is named in server logs.

## Conventions
- **Closed-vocabulary enums** stored as TEXT at the DB layer (e.g. ShareReportStatus, ShareReportReason). `parse()` + `as_str()` round-trip; adding a variant doesn't need a migration.
- **Trait + Postgres impl + Memory impl** pattern for stores (see `share_metadata.rs`, `share_reports.rs`). Memory impl lives in `mod test_support` and feeds route-layer unit tests.
- **Handle validation** via `validate_handle()` in sharing_routes.rs — ASCII alphanumeric + `_-`, ≤ 64 chars. Reuse before any SpiceDB write or DB lookup that takes a handle.
- **Per-share scope** stored as JSONB (`share_metadata.scope`). NULL = legacy `full`. Filtering on `allow`/`deny` event types uses ANY/ALL array predicates.
- **Server-side handle truth.** Sensitive form fields (reporter_handle, recipient_handle for the auth'd user) come off the bearer token / `session.claimedHandle`, never from the client. The client can supply the OTHER party's handle.
- **h1 type plateau (audit v2 §03):** `main h1` baseline = 28px (globals.css). Top-level pages opt to 32px inline. Detail pages can opt to 24px inline. Don't override unless the depth tier calls for it.

## Working Agreements
- **Fact-Forcing Gate hook** fires before every Edit/Write/Bash. Before any tool call, present 4 facts: (1) importers/callers of the target, (2) public symbols affected, (3) data file fields if relevant, (4) verbatim user instruction. The gate is unavoidable in this project — comply rather than fight it. Disable via `ECC_GATEGUARD=off` or `ECC_DISABLED_HOOKS=pre:bash:gateguard-fact-force,pre:edit-write:gateguard-fact-force`.
- **Workspace clippy is `-D warnings`.** Every clippy warning is a CI failure. Workspace-level allows live in `Cargo.toml [workspace.lints.clippy]` with rationale; flip back to deny when a lint starts catching real smells.
- **`cargo fmt --check` is a CI gate.** Run `cargo fmt -p <crate>` after edits.
- **OpenAPI workflow:** edit handler + `#[utoipa::path]` + register in `openapi.rs` paths AND schemas; add `mod` stub to `bin/openapi.rs` for any NEW module; regenerate TS schema via `pnpm --filter api-client-ts run generate`.
- **New `Arc<dyn StoreTrait>` extensions:** mirror the `share_metadata_dyn` pattern in main.rs — Arc, dyn-cast, Extension layer added to the chain at the bottom of `app`.
- **Server bin names:** `starstats-server` (run) and `starstats-server-openapi` (spec gen). NOT `openapi` — that errors.
- **Branch protection on `main`:** rebase, don't merge. Release-manifest auto-commits show up between pushes — `git pull --rebase origin main` then push.

## Do / Don't
- DO mirror existing store patterns (share_metadata → share_reports) when adding a new entity.
- DO write 6-8 store tests against the Memory impl before wiring routes; route-layer tests come next using `tower::ServiceExt::oneshot`.
- DON'T fabricate server-side RSI fetch endpoints to satisfy a UX symmetry — reframe the affordance instead.
- DON'T amend a published commit; rebase + new commit only.
- DON'T `git push --force` to main.
- DON'T re-add ss-eyebrow as decoration. It's a category label above an h2; one per section, not one per card.
