---
name: regen-openapi
description: Regenerate the TypeScript OpenAPI client after touching axum handlers, utoipa schemas, or paths. Runs the spec generator, regenerates packages/api-client-ts/src/generated/schema.ts, and reminds about openapi.rs registration so the spec doesn't drift.
---

# Regenerate OpenAPI Client

Use this when an axum handler in `crates/starstats-server/src/*_routes.rs` has been touched in any of these ways:

- Added/removed a `#[utoipa::path(...)]` annotation
- Added/removed/changed fields on a `ToSchema`-derived DTO
- Added a new module file (`*_routes.rs` or domain crate like `share_reports.rs`)
- Added/removed a route in `main.rs`

If none of those apply, this skill is not needed.

## Checklist

Walk through these in order. Stop and surface anything missing rather than silently inferring.

1. **Schema registration** — open `crates/starstats-server/src/openapi.rs` and confirm:
   - Every new `#[utoipa::path]` handler appears in the `paths(...)` list
   - Every new `ToSchema` DTO appears in the `components(schemas(...))` list
   - For a brand-new module file, the `use crate::<new_module>;` line is present

2. **OpenAPI binary stub** — if a new module file was created:
   - Open `crates/starstats-server/src/bin/openapi.rs`
   - Confirm `#[path = "../<new_module>.rs"] mod <new_module>;` is present
   - The binary won't compile without this stub

3. **Server compiles + clippy clean**:
   ```
   cargo check -p starstats-server
   cargo clippy -p starstats-server --bin starstats-server -- -D warnings
   ```
   Workspace policy is `-D warnings`; CI will fail on any warning.

4. **Regenerate the TS schema**:
   ```
   pnpm --filter api-client-ts run generate
   ```
   The script spawns `cargo run --bin starstats-server-openapi`, captures stdout JSON, and writes `packages/api-client-ts/src/generated/schema.ts`. The bin name is `starstats-server-openapi`, NOT `openapi` — that errors with "no bin target".

5. **Diff the generated schema** and confirm the changes match intent:
   ```
   git diff packages/api-client-ts/src/generated/schema.ts
   ```
   Watch for:
   - New `paths` entries — should match the new route
   - New `components.schemas` entries — should include every new DTO
   - NO unexpected deletions in unrelated paths (signals a missing `paths(...)` entry that pruned existing routes)

6. **Add client wrapper functions** in `apps/web/src/lib/api.ts`:
   - One `export async function ...` per new route, modelled on the closest existing function (e.g. `getAdminSharingOverview` for a moderator-gated GET).
   - Type alias the schema reference: `export type Foo = apiSchema['schemas']['Foo'];`

7. **Web typechecks**:
   ```
   pnpm --filter web typecheck
   ```

## Common failure modes

- **"no bin target named openapi"** → use `starstats-server-openapi`, not `openapi`.
- **`schema.ts` regenerated but missing new paths/DTOs** → forgot step 1 (didn't register in `openapi.rs`).
- **`bin/openapi.rs` build error** → forgot step 2 (didn't add the `#[path]` stub for the new module).
- **`schema.ts` regenerated but Web typecheck fails on the new path** → openapi-typescript emitted a type the consumer didn't catch; usually a `null` vs missing-key difference. Mirror the existing client function's `body` shape.
