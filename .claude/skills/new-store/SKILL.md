---
name: new-store
description: Scaffold a new Postgres-backed store in starstats-server following the share_metadata / share_reports pattern (trait + Postgres impl + Memory test impl + baseline tests + main.rs wiring stub). Pass the entity name as the argument, e.g. `/new-store ShareSubscription`.
disable-model-invocation: true
---

# New Store (Postgres-backed entity)

Use this to add a new domain entity that lives in its own crate file
under `crates/starstats-server/src/` with the canonical
trait + Postgres impl + Memory impl + tests pattern.

Reference implementations to mirror:
- `crates/starstats-server/src/share_metadata.rs` (simpler — no enums)
- `crates/starstats-server/src/share_reports.rs` (with closed enums, rate-limit count, audit emission)

## Arguments

`$1` = PascalCase entity name (e.g. `ShareSubscription`, `HangarSnapshotV2`)

Derived names used below:
- Module / file: `snake_case` -> `share_subscription` -> `share_subscription.rs`
- Store trait: `<Entity>Store` -> `ShareSubscriptionStore`
- Postgres impl: `Postgres<Entity>Store`
- Memory test impl: `Memory<Entity>Store`
- Error: `<Entity>Error`
- Table name: `<snake_case>s` (plural)

## Checklist

1. **Plan the table.** Sketch the SQL columns BEFORE coding. For each:
   - Decide the storage type (TEXT for closed-vocabulary enums, UUID for PKs via `gen_random_uuid()`, TIMESTAMPTZ for instants).
   - Confirm every column is nullable OR has a DEFAULT (additive-migration rule from CLAUDE.md).
   - Identify the index access patterns up front — at minimum the queue index (status, created_at DESC) for moderated entities.

2. **Write the migration** as the next-numbered file:
   ```
   crates/starstats-server/migrations/00NN_<snake_case>s.sql
   ```
   - `CREATE TABLE IF NOT EXISTS ...`
   - `CREATE INDEX IF NOT EXISTS ...` for every documented access pattern
   - Header comment naming the audit-v2 section + the application-layer vocabulary (status / reason enums)

3. **Write `crates/starstats-server/src/<snake_case>.rs`** following the share_reports template:
   - Module-level doc explaining the trust boundary, the audit-action vocabulary the entity emits, and migration coupling.
   - Closed-vocabulary `enum` types with `as_str() -> &'static str` and `parse(&str) -> Option<Self>`.
   - Row struct `<Entity>` with `Serialize + Deserialize + ToSchema`.
   - `#[derive(thiserror)] enum <Entity>Error` covering `Database(sqlx::Error)`, `NotFound`, `AlreadyResolved` (or analogue), `Domain(String)`.
   - `#[async_trait] pub trait <Entity>Store: Send + Sync + 'static { ... }` — methods named for the CALLER's intent, not the SQL (`list_open`, `resolve`, `count_recent_by_<x>`).
   - Trait method docs say WHY (`/// Used by the X handler for Y decision`), not what the SQL does.
   - `pub struct Postgres<Entity>Store { pool: PgPool }` — `query_as` over a flat tuple, then `row_to_<entity>(row) -> Result<Entity, Error>` that parses enums.
   - `#[cfg(test)] pub mod test_support { ... Memory<Entity>Store ... }` — `Mutex<Vec<Entity>>` backing, mirrors trait behavior, NOT a no-op shell.
   - Inline `#[cfg(test)] mod tests` with the baseline suite: enum round-trips, create+get, list+filter+order, double-resolve rejection (if applicable), case-insensitive lookup (if applicable). Aim for 6-8 tests.

4. **Wire `main.rs`**:
   ```rust
   use crate::<snake_case>::Postgres<Entity>Store;
   ...
   mod <snake_case>;
   ...
   let <snake_case>s: Arc<Postgres<Entity>Store> =
       Arc::new(Postgres<Entity>Store::new(pool.clone()));
   ...
   let <snake_case>s_dyn: Arc<dyn crate::<snake_case>::<Entity>Store> =
       <snake_case>s.clone();
   ...
   .layer(Extension(<snake_case>s_dyn))
   ```

5. **Wire `bin/openapi.rs`** with a `#[path = "../<snake_case>.rs"] mod <snake_case>;` stub (skip if the entity has no `ToSchema` types — but most do).

6. **Test**:
   ```
   cargo test -p starstats-server --bin starstats-server <snake_case>
   cargo clippy -p starstats-server --bin starstats-server -- -D warnings
   cargo fmt -p starstats-server
   ```

7. **Handlers come next** (separate skill / file). The store is the contract; handlers `Extension(Arc<dyn ...>)` it.

## DON'T

- Don't add methods to the trait "for future use" — `#[allow(dead_code)]` accumulates noise. Add when the first handler needs it.
- Don't store enums as integer codes or arbitrary strings without parse/as_str round-trip tests.
- Don't write the Postgres impl before the Memory impl — the Memory impl is the contract, and the trait method shapes get clearer when you write the test fixture first.
