//! Persistent store for the Star Citizen class-name reference catalogue.
//!
//! The store is keyed on `(category, class_name)` — the internal Star
//! Citizen identifier embedded in event payloads, scoped to the kind
//! of entity it refers to (vehicle, weapon, item, location). The daily
//! refresh job pulls each category via the upstream wiki API and upserts
//! every entry; render paths read by `(category, class_name)`
//! case-insensitive to translate raw events into player-friendly metadata.
//!
//! Trait shape: implementers only need the three generic methods
//! (`upsert_entries`, `get_entry`, `list_category`). The legacy
//! vehicle-specific methods are default impls that delegate to the
//! generic ones plus a small ReferenceEntry ↔ VehicleReference
//! conversion — keeps existing callers and tests working through the
//! transition without forcing every implementer to maintain two
//! parallel code paths.
//!
//! Errors collapse into a single [`ReferenceStoreError::Backend`]
//! variant. The route layer treats backend failure as a 503 either
//! way, and there are no unique-constraint races worth carving out a
//! richer taxonomy for. Shape mirrors [`crate::profile_store::ProfileStoreError`].

use crate::reference_data::{ReferenceCategory, ReferenceEntry, VehicleReference};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Admin dashboard summary for one reference category. `entry_count`
/// is the row count in `reference_registry` filtered by category;
/// `latest_updated_at` is `MAX(updated_at)` (NULL when the category
/// has no rows yet, e.g. a freshly-added one whose cron hasn't run).
#[derive(Debug, Clone)]
pub struct CategorySummary {
    pub category: ReferenceCategory,
    pub entry_count: i64,
    pub latest_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReferenceStoreError {
    #[error("reference store backend error: {0}")]
    Backend(String),
}

impl From<sqlx::Error> for ReferenceStoreError {
    fn from(err: sqlx::Error) -> Self {
        Self::Backend(err.to_string())
    }
}

#[async_trait]
pub trait ReferenceStore: Send + Sync + 'static {
    /// Upsert each entry by (category, class_name). Returns the count
    /// of rows affected. Idempotent — repeated calls with the same
    /// payload are cheap and safe.
    async fn upsert_entries(
        &self,
        entries: &[ReferenceEntry],
    ) -> Result<usize, ReferenceStoreError>;

    /// Look up a single entry by (category, class_name). Case-insensitive
    /// on `class_name` — game logs occasionally vary case on the same
    /// class.
    async fn get_entry(
        &self,
        category: ReferenceCategory,
        class_name: &str,
    ) -> Result<Option<ReferenceEntry>, ReferenceStoreError>;

    /// Full list for a category, ordered by `class_name` ASC.
    async fn list_category(
        &self,
        category: ReferenceCategory,
    ) -> Result<Vec<ReferenceEntry>, ReferenceStoreError>;

    /// Admin-only: row counts + latest update timestamp per category.
    /// Cheap aggregate over the registry; the admin dashboard uses
    /// this to surface stale categories at a glance.
    async fn category_summaries(
        &self,
    ) -> Result<Vec<CategorySummary>, ReferenceStoreError> {
        // Default impl: walk each category via list_category. Slow
        // but correct for the in-memory test store; the Postgres
        // impl overrides with a single GROUP BY aggregate.
        let mut out = Vec::new();
        for c in [
            ReferenceCategory::Vehicle,
            ReferenceCategory::Weapon,
            ReferenceCategory::Item,
            ReferenceCategory::Location,
        ] {
            let entries = self.list_category(c).await?;
            out.push(CategorySummary {
                category: c,
                entry_count: entries.len() as i64,
                latest_updated_at: None,
            });
        }
        Ok(out)
    }

    // -- Legacy vehicle-shaped helpers ---------------------------------
    //
    // Default impls delegate to the generic methods + per-category
    // conversion. Implementers don't need to override these. The
    // in-tree cron no longer calls `upsert_vehicles` after P3, but
    // the method stays on the trait for backwards compatibility with
    // external implementers and the existing tests.

    #[allow(dead_code)]
    async fn upsert_vehicles(
        &self,
        vehicles: &[VehicleReference],
    ) -> Result<usize, ReferenceStoreError> {
        let entries: Vec<ReferenceEntry> = vehicles.iter().map(vehicle_to_entry).collect();
        self.upsert_entries(&entries).await
    }

    async fn get_vehicle(
        &self,
        class_name: &str,
    ) -> Result<Option<VehicleReference>, ReferenceStoreError> {
        Ok(self
            .get_entry(ReferenceCategory::Vehicle, class_name)
            .await?
            .map(entry_to_vehicle))
    }

    async fn list_vehicles(&self) -> Result<Vec<VehicleReference>, ReferenceStoreError> {
        Ok(self
            .list_category(ReferenceCategory::Vehicle)
            .await?
            .into_iter()
            .map(entry_to_vehicle)
            .collect())
    }
}

/// `ReferenceEntry` (generic) → typed `VehicleReference` view. Used by
/// the legacy `get_vehicle` / `list_vehicles` path.
pub(crate) fn entry_to_vehicle(e: ReferenceEntry) -> VehicleReference {
    let meta = e.metadata.as_object().cloned().unwrap_or_default();
    let s = |k: &str| {
        meta.get(k)
            .and_then(serde_json::Value::as_str)
            .map(String::from)
    };
    VehicleReference {
        class_name: e.class_name,
        display_name: e.display_name,
        manufacturer: s("manufacturer"),
        role: s("role"),
        hull_size: s("hull_size"),
        focus: s("focus"),
    }
}

/// Typed `VehicleReference` → generic `ReferenceEntry`. Collapses the
/// typed columns into a metadata JSON object, dropping `None` fields
/// so they don't pollute the JSONB body with explicit `null`s.
#[allow(dead_code)]
pub(crate) fn vehicle_to_entry(v: &VehicleReference) -> ReferenceEntry {
    let mut meta = serde_json::Map::new();
    let mut put = |k: &str, val: &Option<String>| {
        if let Some(s) = val {
            meta.insert(k.into(), serde_json::Value::String(s.clone()));
        }
    };
    put("manufacturer", &v.manufacturer);
    put("role", &v.role);
    put("hull_size", &v.hull_size);
    put("focus", &v.focus);
    ReferenceEntry {
        category: ReferenceCategory::Vehicle,
        class_name: v.class_name.clone(),
        display_name: v.display_name.clone(),
        metadata: serde_json::Value::Object(meta),
    }
}

// -- Postgres impl ---------------------------------------------------
//
// `updated_at` and `source` are intentionally omitted from the
// surfaced `SELECT` columns. `updated_at` drives ops alerting
// (stale-cache detection); `source` is provenance for debugging the
// refresh path. Neither is part of the public response shape.

pub struct PostgresReferenceStore {
    pool: PgPool,
}

impl PostgresReferenceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReferenceStore for PostgresReferenceStore {
    async fn upsert_entries(
        &self,
        entries: &[ReferenceEntry],
    ) -> Result<usize, ReferenceStoreError> {
        // Wrap the batch in a transaction so a partial failure (e.g. a
        // constraint violation midway) rolls back cleanly. The caller
        // treats "fresh refresh" as all-or-nothing — partial writes
        // would leave the cache in an inconsistent state where some
        // rows point at last-week's metadata and others at today's.
        let mut tx = self.pool.begin().await?;
        let mut affected: u64 = 0;

        for e in entries {
            let result = sqlx::query(
                r#"
                INSERT INTO reference_registry
                    (category, class_name, display_name, metadata, source, updated_at)
                VALUES ($1, $2, $3, $4, $5, NOW())
                ON CONFLICT (category, class_name) DO UPDATE
                    SET display_name = EXCLUDED.display_name,
                        metadata     = EXCLUDED.metadata,
                        source       = EXCLUDED.source,
                        updated_at   = NOW()
                "#,
            )
            .bind(e.category.as_str())
            .bind(&e.class_name)
            .bind(&e.display_name)
            .bind(&e.metadata)
            .bind("wiki_api")
            .execute(&mut *tx)
            .await?;
            affected = affected.saturating_add(result.rows_affected());
        }

        tx.commit().await?;
        Ok(affected as usize)
    }

    async fn get_entry(
        &self,
        category: ReferenceCategory,
        class_name: &str,
    ) -> Result<Option<ReferenceEntry>, ReferenceStoreError> {
        let row: Option<(String, String, serde_json::Value)> = sqlx::query_as(
            "SELECT class_name, display_name, metadata \
                 FROM reference_registry \
                 WHERE category = $1 AND lower(class_name) = lower($2)",
        )
        .bind(category.as_str())
        .bind(class_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(
            row.map(|(class_name, display_name, metadata)| ReferenceEntry {
                category,
                class_name,
                display_name,
                metadata,
            }),
        )
    }

    async fn list_category(
        &self,
        category: ReferenceCategory,
    ) -> Result<Vec<ReferenceEntry>, ReferenceStoreError> {
        let rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
            "SELECT class_name, display_name, metadata \
                 FROM reference_registry \
                 WHERE category = $1 \
                 ORDER BY class_name ASC",
        )
        .bind(category.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(class_name, display_name, metadata)| ReferenceEntry {
                category,
                class_name,
                display_name,
                metadata,
            })
            .collect())
    }

    async fn category_summaries(
        &self,
    ) -> Result<Vec<CategorySummary>, ReferenceStoreError> {
        // Single GROUP BY beats 4 list_category round-trips. Outer
        // LEFT JOIN against the static category list keeps every
        // category present even when it has no rows yet, which
        // matters for "is the location sync running?" diagnostics.
        let rows: Vec<(String, i64, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT cat, COALESCE(cnt, 0), latest
             FROM unnest(ARRAY['vehicle','weapon','item','location']) AS cat
             LEFT JOIN (
                 SELECT category, COUNT(*) AS cnt, MAX(updated_at) AS latest
                 FROM reference_registry
                 GROUP BY category
             ) agg ON agg.category = cat",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for (cat_str, count, latest) in rows {
            let Some(category) = ReferenceCategory::parse(&cat_str) else {
                tracing::warn!(category = %cat_str, "unknown reference category in summary");
                continue;
            };
            out.push(CategorySummary {
                category,
                entry_count: count,
                latest_updated_at: latest,
            });
        }
        Ok(out)
    }
}

// -- Test impl + tests -----------------------------------------------

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory implementation used by handler-level tests. Mirrors
    /// Postgres semantics: idempotent upsert keyed on
    /// (category, lower(class_name)), case-insensitive lookup,
    /// ASCII-sorted list per category.
    #[derive(Default)]
    pub struct MemoryReferenceStore {
        rows: Mutex<HashMap<(&'static str, String), ReferenceEntry>>,
    }

    impl MemoryReferenceStore {
        pub fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl ReferenceStore for MemoryReferenceStore {
        async fn upsert_entries(
            &self,
            entries: &[ReferenceEntry],
        ) -> Result<usize, ReferenceStoreError> {
            let mut rows = self.rows.lock().unwrap();
            let mut affected = 0usize;
            for e in entries {
                let key = (e.category.as_str(), e.class_name.to_lowercase());
                rows.insert(key, e.clone());
                affected = affected.saturating_add(1);
            }
            Ok(affected)
        }

        async fn get_entry(
            &self,
            category: ReferenceCategory,
            class_name: &str,
        ) -> Result<Option<ReferenceEntry>, ReferenceStoreError> {
            let rows = self.rows.lock().unwrap();
            Ok(rows
                .get(&(category.as_str(), class_name.to_lowercase()))
                .cloned())
        }

        async fn list_category(
            &self,
            category: ReferenceCategory,
        ) -> Result<Vec<ReferenceEntry>, ReferenceStoreError> {
            let rows = self.rows.lock().unwrap();
            let mut out: Vec<ReferenceEntry> = rows
                .iter()
                .filter(|((cat, _), _)| *cat == category.as_str())
                .map(|(_, v)| v.clone())
                .collect();
            out.sort_by(|a, b| a.class_name.cmp(&b.class_name));
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MemoryReferenceStore;
    use super::*;

    fn make_vehicle(class_name: &str, display_name: &str) -> VehicleReference {
        VehicleReference {
            class_name: class_name.to_owned(),
            display_name: display_name.to_owned(),
            manufacturer: Some("Aegis Dynamics".to_owned()),
            role: Some("Heavy Fighter".to_owned()),
            hull_size: Some("Small".to_owned()),
            focus: Some("Combat".to_owned()),
        }
    }

    #[tokio::test]
    async fn upsert_and_get_round_trips() {
        let store = MemoryReferenceStore::new();
        let v = make_vehicle("AEGS_Avenger_Stalker", "Aegis Avenger Stalker");
        let affected = store.upsert_vehicles(&[v.clone()]).await.unwrap();
        assert_eq!(affected, 1);

        let got = store
            .get_vehicle("AEGS_Avenger_Stalker")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, v);

        let updated = VehicleReference {
            display_name: "Aegis Avenger Stalker (Refreshed)".to_owned(),
            ..v.clone()
        };
        store.upsert_vehicles(&[updated.clone()]).await.unwrap();
        let got = store
            .get_vehicle("AEGS_Avenger_Stalker")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.display_name, "Aegis Avenger Stalker (Refreshed)");

        assert!(store
            .get_vehicle("not-a-real-class")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn get_vehicle_is_case_insensitive() {
        let store = MemoryReferenceStore::new();
        let v = make_vehicle("AEGS_Avenger_Stalker", "Aegis Avenger Stalker");
        store.upsert_vehicles(&[v.clone()]).await.unwrap();

        let lower = store
            .get_vehicle("aegs_avenger_stalker")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lower, v);

        let upper = store
            .get_vehicle("AEGS_AVENGER_STALKER")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(upper, v);
    }

    #[tokio::test]
    async fn list_vehicles_orders_by_class_name_ascending() {
        let store = MemoryReferenceStore::new();
        store
            .upsert_vehicles(&[
                make_vehicle("DRAK_Cutlass_Black", "Drake Cutlass Black"),
                make_vehicle("AEGS_Avenger_Stalker", "Aegis Avenger Stalker"),
                make_vehicle("ANVL_Hornet_F7C", "Anvil Hornet F7C"),
            ])
            .await
            .unwrap();

        let listed = store.list_vehicles().await.unwrap();
        let class_names: Vec<&str> = listed.iter().map(|v| v.class_name.as_str()).collect();
        assert_eq!(
            class_names,
            vec![
                "AEGS_Avenger_Stalker",
                "ANVL_Hornet_F7C",
                "DRAK_Cutlass_Black"
            ]
        );
    }

    #[tokio::test]
    async fn generic_entries_are_scoped_by_category() {
        let store = MemoryReferenceStore::new();
        let mut meta = serde_json::Map::new();
        meta.insert(
            "damage_type".into(),
            serde_json::Value::String("Energy".into()),
        );
        let weapon = ReferenceEntry {
            category: ReferenceCategory::Weapon,
            class_name: "KLWE_LaserCannon_S2".to_owned(),
            display_name: "Klaus & Werner Sledge II".to_owned(),
            metadata: serde_json::Value::Object(meta),
        };
        store.upsert_entries(&[weapon.clone()]).await.unwrap();

        // Same class_name under a different category must not collide.
        let vehicle_with_same_id = ReferenceEntry {
            category: ReferenceCategory::Vehicle,
            class_name: "KLWE_LaserCannon_S2".to_owned(),
            display_name: "(theoretical) some other thing".to_owned(),
            metadata: serde_json::Value::Object(Default::default()),
        };
        store
            .upsert_entries(&[vehicle_with_same_id.clone()])
            .await
            .unwrap();

        let got_weapon = store
            .get_entry(ReferenceCategory::Weapon, "klwe_lasercannon_s2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got_weapon.display_name, "Klaus & Werner Sledge II");

        let got_vehicle = store
            .get_entry(ReferenceCategory::Vehicle, "KLWE_LaserCannon_S2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got_vehicle.display_name, "(theoretical) some other thing");

        // Cross-category lookup returns None.
        assert!(store
            .get_entry(ReferenceCategory::Item, "KLWE_LaserCannon_S2")
            .await
            .unwrap()
            .is_none());
    }
}
