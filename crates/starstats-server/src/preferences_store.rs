//! Persistent store for the user's UI preferences (theme + future
//! forward-extensible toggles).
//!
//! Preferences live on the existing `users` row as a JSONB column
//! (`users.preferences`, default `'{}'::jsonb`) rather than a separate
//! table because:
//!
//!  - The set is small and per-user (theme today; notification toggles
//!    + accent intensity + name plate later) — a satellite table would
//!    be all overhead.
//!  - The `users` row already exists by the time anyone calls these
//!    endpoints (auth gate guarantees it), so PUT is a plain UPDATE
//!    and never has to INSERT.
//!  - JSONB lets us evolve the schema without a migration per field.
//!
//! Mirrors the trait-fronted shape of [`crate::hangar_store`]: a
//! Postgres impl for production and a [`MemoryPreferencesStore`] under
//! `test_support` for handler-level tests. A single
//! [`sqlx::Error`] is surfaced to the route layer as a 500.

use async_trait::async_trait;
use sqlx::PgPool;
use starstats_core::wire::UserPreferences;
use uuid::Uuid;

#[async_trait]
pub trait PreferencesStore: Send + Sync + 'static {
    /// Fetch the caller's preferences. Returns `UserPreferences::default()`
    /// when the column is `'{}'::jsonb` or has no fields set — callers
    /// never have to special-case "no row stored".
    async fn get(&self, user_id: Uuid) -> Result<UserPreferences, sqlx::Error>;

    /// Replace the caller's preferences in full. The user row is
    /// guaranteed to exist (auth gate), so this is a plain UPDATE
    /// rather than an upsert.
    async fn put(&self, user_id: Uuid, prefs: &UserPreferences) -> Result<(), sqlx::Error>;
}

// -- Postgres impl ---------------------------------------------------

pub struct PostgresPreferencesStore {
    pool: PgPool,
}

impl PostgresPreferencesStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PreferencesStore for PostgresPreferencesStore {
    async fn get(&self, user_id: Uuid) -> Result<UserPreferences, sqlx::Error> {
        // `sqlx::types::Json<UserPreferences>` decodes the JSONB column
        // through serde — same pattern as `hangar_store::get_snapshot`.
        // A row that doesn't exist (deleted account, racing handlers)
        // is returned as `RowNotFound` and propagated; the route maps
        // that to 500. An empty `'{}'` JSONB decodes cleanly to
        // `UserPreferences::default()` because every field is optional.
        let row: (sqlx::types::Json<UserPreferences>,) =
            sqlx::query_as("SELECT preferences FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0 .0)
    }

    async fn put(&self, user_id: Uuid, prefs: &UserPreferences) -> Result<(), sqlx::Error> {
        // Whole-document replace. The wire type's `skip_serializing_if`
        // keeps absent fields out of the JSONB payload, so an empty
        // `UserPreferences` writes back as `'{}'::jsonb` — matching the
        // column default and keeping later GETs cheap.
        sqlx::query("UPDATE users SET preferences = $1::jsonb WHERE id = $2")
            .bind(sqlx::types::Json(prefs))
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

// -- Test impl + tests -----------------------------------------------

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory implementation used by handler-level tests. Mirrors
    /// the Postgres semantics: GET returns `UserPreferences::default()`
    /// when nothing is stored (the production code reads `'{}'::jsonb`
    /// which decodes to default), and PUT replaces in full.
    #[derive(Default)]
    pub struct MemoryPreferencesStore {
        prefs: Mutex<HashMap<Uuid, UserPreferences>>,
    }

    impl MemoryPreferencesStore {
        pub fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl PreferencesStore for MemoryPreferencesStore {
        async fn get(&self, user_id: Uuid) -> Result<UserPreferences, sqlx::Error> {
            Ok(self
                .prefs
                .lock()
                .unwrap()
                .get(&user_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn put(&self, user_id: Uuid, prefs: &UserPreferences) -> Result<(), sqlx::Error> {
            self.prefs.lock().unwrap().insert(user_id, prefs.clone());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MemoryPreferencesStore;
    use super::*;

    #[tokio::test]
    async fn get_returns_default_when_nothing_stored() {
        let store = MemoryPreferencesStore::new();
        let user = Uuid::new_v4();

        let got = store.get(user).await.unwrap();
        // `UserPreferences::default()` -> theme is None. The route
        // layer relies on this so callers never see a 404 for a
        // "no preferences yet" state.
        assert_eq!(got, UserPreferences::default());
        assert!(got.theme.is_none());
    }

    #[tokio::test]
    async fn put_then_get_round_trips() {
        let store = MemoryPreferencesStore::new();
        let user = Uuid::new_v4();
        let prefs = UserPreferences {
            theme: Some("pyro".into()),
        };

        store.put(user, &prefs).await.unwrap();
        let got = store.get(user).await.unwrap();
        assert_eq!(got, prefs);
        assert_eq!(got.theme.as_deref(), Some("pyro"));
    }

    #[tokio::test]
    async fn put_overwrites_previous_prefs() {
        let store = MemoryPreferencesStore::new();
        let user = Uuid::new_v4();

        store
            .put(
                user,
                &UserPreferences {
                    theme: Some("stanton".into()),
                },
            )
            .await
            .unwrap();
        store
            .put(
                user,
                &UserPreferences {
                    theme: Some("nyx".into()),
                },
            )
            .await
            .unwrap();

        let got = store.get(user).await.unwrap();
        assert_eq!(got.theme.as_deref(), Some("nyx"));
    }

    #[tokio::test]
    async fn put_with_default_clears_fields() {
        let store = MemoryPreferencesStore::new();
        let user = Uuid::new_v4();

        store
            .put(
                user,
                &UserPreferences {
                    theme: Some("terra".into()),
                },
            )
            .await
            .unwrap();
        // Empty payload — the wire type's skip_serializing_if means
        // this round-trips through Postgres as `'{}'::jsonb`.
        store.put(user, &UserPreferences::default()).await.unwrap();

        let got = store.get(user).await.unwrap();
        assert!(got.theme.is_none());
    }
}
