//! Persistent store for Star Citizen vehicle reference data.
//!
//! The cache is keyed on `class_name` — the internal Star Citizen
//! identifier embedded in event payloads. The daily refresh job
//! pulls the catalogue via [`crate::reference_data::ReferenceClient`]
//! and upserts every row through this store; render paths read by
//! `class_name` (case-insensitive) to translate raw events into
//! player-friendly metadata.
//!
//! Errors collapse into a single [`ReferenceStoreError::Backend`]
//! variant. The route layer treats backend failure as a 503 either
//! way, and there are no unique-constraint races worth carving out
//! a richer taxonomy for (the only constraint here is the primary
//! key on `class_name`, which the upsert resolves by definition).
//! The shape mirrors [`crate::profile_store::ProfileStoreError`].

use crate::reference_data::VehicleReference;
use async_trait::async_trait;
use sqlx::PgPool;

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
    /// Upsert each vehicle by class_name. Returns the count of rows
    /// affected (insert + update combined). The store is idempotent —
    /// repeated calls with the same data are cheap and safe.
    async fn upsert_vehicles(
        &self,
        vehicles: &[VehicleReference],
    ) -> Result<usize, ReferenceStoreError>;

    /// Look up by class_name. Match is case-insensitive — game logs
    /// occasionally vary case on the same class.
    async fn get_vehicle(
        &self,
        class_name: &str,
    ) -> Result<Option<VehicleReference>, ReferenceStoreError>;

    /// Full list, ordered by class_name ASC. ~150 entries, fits in a
    /// single response — no pagination needed.
    async fn list_vehicles(&self) -> Result<Vec<VehicleReference>, ReferenceStoreError>;
}

// -- Postgres impl ---------------------------------------------------
//
// `updated_at` is intentionally omitted from the surfaced `SELECT`
// columns: it drives ops alerting (stale-cache detection) but isn't
// part of the public response shape.

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
    async fn upsert_vehicles(
        &self,
        vehicles: &[VehicleReference],
    ) -> Result<usize, ReferenceStoreError> {
        // Wrap the batch in a transaction so a partial failure (e.g.
        // a constraint violation midway) rolls back cleanly. The
        // caller treats "fresh refresh" as all-or-nothing — partial
        // writes would leave the cache in an inconsistent state where
        // some vehicles point at last-week's metadata and others at
        // today's.
        let mut tx = self.pool.begin().await?;
        let mut affected: u64 = 0;

        for v in vehicles {
            let result = sqlx::query(
                r#"
                INSERT INTO vehicle_reference
                    (class_name, display_name, manufacturer, role, hull_size, focus, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, NOW())
                ON CONFLICT (class_name) DO UPDATE
                    SET display_name = EXCLUDED.display_name,
                        manufacturer = EXCLUDED.manufacturer,
                        role         = EXCLUDED.role,
                        hull_size    = EXCLUDED.hull_size,
                        focus        = EXCLUDED.focus,
                        updated_at   = NOW()
                "#,
            )
            .bind(&v.class_name)
            .bind(&v.display_name)
            .bind(&v.manufacturer)
            .bind(&v.role)
            .bind(&v.hull_size)
            .bind(&v.focus)
            .execute(&mut *tx)
            .await?;
            affected = affected.saturating_add(result.rows_affected());
        }

        tx.commit().await?;
        Ok(affected as usize)
    }

    async fn get_vehicle(
        &self,
        class_name: &str,
    ) -> Result<Option<VehicleReference>, ReferenceStoreError> {
        // Case-insensitive lookup — matches the
        // `vehicle_reference_class_lower_idx` index in the migration.
        let row: Option<(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT class_name, display_name, manufacturer, role, hull_size, focus \
                 FROM vehicle_reference \
                 WHERE lower(class_name) = lower($1)",
        )
        .bind(class_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_vehicle))
    }

    async fn list_vehicles(&self) -> Result<Vec<VehicleReference>, ReferenceStoreError> {
        let rows: Vec<(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT class_name, display_name, manufacturer, role, hull_size, focus \
                 FROM vehicle_reference \
                 ORDER BY class_name ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_vehicle).collect())
    }
}

fn row_to_vehicle(
    (class_name, display_name, manufacturer, role, hull_size, focus): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ),
) -> VehicleReference {
    VehicleReference {
        class_name,
        display_name,
        manufacturer,
        role,
        hull_size,
        focus,
    }
}

// -- Test impl + tests -----------------------------------------------

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory implementation used by handler-level tests. Mirrors
    /// the Postgres semantics: idempotent upsert keyed on
    /// `class_name`, case-insensitive lookup, ASCII-sorted list.
    /// Internal map is keyed by lowercase `class_name` so the lookup
    /// matches the Postgres `lower(class_name)` index without
    /// scanning every row.
    #[derive(Default)]
    pub struct MemoryReferenceStore {
        rows: Mutex<HashMap<String, VehicleReference>>,
    }

    impl MemoryReferenceStore {
        pub fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl ReferenceStore for MemoryReferenceStore {
        async fn upsert_vehicles(
            &self,
            vehicles: &[VehicleReference],
        ) -> Result<usize, ReferenceStoreError> {
            let mut rows = self.rows.lock().unwrap();
            let mut affected = 0usize;
            for v in vehicles {
                rows.insert(v.class_name.to_lowercase(), v.clone());
                affected = affected.saturating_add(1);
            }
            Ok(affected)
        }

        async fn get_vehicle(
            &self,
            class_name: &str,
        ) -> Result<Option<VehicleReference>, ReferenceStoreError> {
            let rows = self.rows.lock().unwrap();
            Ok(rows.get(&class_name.to_lowercase()).cloned())
        }

        async fn list_vehicles(&self) -> Result<Vec<VehicleReference>, ReferenceStoreError> {
            let rows = self.rows.lock().unwrap();
            // Sort by `class_name` ASC to match the Postgres
            // `ORDER BY class_name ASC` clause. Rust string ordering
            // is lexicographic over bytes, which lines up with
            // Postgres's default text collation for ASCII identifiers.
            let mut out: Vec<VehicleReference> = rows.values().cloned().collect();
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

        // Re-upserting the same row is idempotent — the latest values
        // win and the affected count reflects the call (not "0 because
        // nothing changed"), mirroring Postgres' `ON CONFLICT … DO
        // UPDATE` which always reports a touched row.
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

        // Missing class_name resolves to None rather than erroring.
        assert!(store
            .get_vehicle("not-a-real-class")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn get_vehicle_is_case_insensitive() {
        // Game logs occasionally vary case on the same class — store
        // it Pascal-cased and look it up snake- or upper-cased to
        // prove the lookup ignores case end to end.
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
        // Insert out of order — list_vehicles must reorder.
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
                "DRAK_Cutlass_Black",
            ]
        );
    }
}
