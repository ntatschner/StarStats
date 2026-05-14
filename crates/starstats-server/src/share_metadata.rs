//! Per-user share metadata — expiry timestamps + human notes.
//!
//! SpiceDB is the source of truth for whether a share exists. This
//! module owns the small Postgres side-table that pairs each
//! (owner_handle, recipient_handle) with optional `expires_at` and
//! `note` fields, so callers can:
//!
//!  * surface "expires in 7 days" / "for the dev cell" annotations
//!    in the UI without inventing schema on top of SpiceDB,
//!  * enforce expiry at read-time in the friend_* handlers (4xx +
//!    lazy SpiceDB row deletion when `expires_at < now()`).
//!
//! The table is keyed on lower-cased handle pairs so the (case-
//! preserved) display handles in SpiceDB collapse to a single row.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// One metadata row. Both `expires_at` and `note` are nullable to
/// model "share exists, no extras".
#[derive(Debug, Clone)]
pub struct ShareMeta {
    pub owner_handle: String,
    pub recipient_handle: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShareMetaError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Maximum length of the optional note. Validation lives in the
/// HTTP handler before this trait is ever called.
pub const NOTE_MAX_LEN: usize = 280;

#[async_trait]
pub trait ShareMetadataStore: Send + Sync + 'static {
    /// Upsert a metadata row. Idempotent — re-granting a share
    /// overwrites the previous expiry/note.
    async fn upsert(
        &self,
        owner_handle: &str,
        recipient_handle: &str,
        expires_at: Option<DateTime<Utc>>,
        note: Option<&str>,
    ) -> Result<(), ShareMetaError>;

    /// Look up a single metadata row. `Ok(None)` = "share exists,
    /// no metadata recorded" which the handler treats as no expiry
    /// and no note.
    async fn find(
        &self,
        owner_handle: &str,
        recipient_handle: &str,
    ) -> Result<Option<ShareMeta>, ShareMetaError>;

    /// Bulk lookup for one owner — pairs with `list_shares`.
    async fn list_by_owner(
        &self,
        owner_handle: &str,
    ) -> Result<Vec<ShareMeta>, ShareMetaError>;

    /// Bulk lookup for one recipient — pairs with
    /// `list_shared_with_me`.
    async fn list_by_recipient(
        &self,
        recipient_handle: &str,
    ) -> Result<Vec<ShareMeta>, ShareMetaError>;

    /// Delete the metadata row when its SpiceDB relation is
    /// revoked. Idempotent — "no row" is a successful outcome.
    async fn delete(
        &self,
        owner_handle: &str,
        recipient_handle: &str,
    ) -> Result<(), ShareMetaError>;
}

pub struct PostgresShareMetadataStore {
    pool: PgPool,
}

impl PostgresShareMetadataStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ShareMetadataStore for PostgresShareMetadataStore {
    async fn upsert(
        &self,
        owner_handle: &str,
        recipient_handle: &str,
        expires_at: Option<DateTime<Utc>>,
        note: Option<&str>,
    ) -> Result<(), ShareMetaError> {
        // ON CONFLICT mirrors the expression-based PK declaration.
        sqlx::query(
            r#"
            INSERT INTO share_metadata
                (owner_handle, recipient_handle, expires_at, note)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (lower(owner_handle), lower(recipient_handle))
            DO UPDATE SET
                expires_at = EXCLUDED.expires_at,
                note       = EXCLUDED.note
            "#,
        )
        .bind(owner_handle)
        .bind(recipient_handle)
        .bind(expires_at)
        .bind(note)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find(
        &self,
        owner_handle: &str,
        recipient_handle: &str,
    ) -> Result<Option<ShareMeta>, ShareMetaError> {
        let row = sqlx::query_as::<
            _,
            (String, String, Option<DateTime<Utc>>, Option<String>, DateTime<Utc>),
        >(
            r#"
            SELECT owner_handle, recipient_handle, expires_at, note, created_at
            FROM share_metadata
            WHERE lower(owner_handle) = lower($1)
              AND lower(recipient_handle) = lower($2)
            "#,
        )
        .bind(owner_handle)
        .bind(recipient_handle)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(o, r, e, n, c)| ShareMeta {
            owner_handle: o,
            recipient_handle: r,
            expires_at: e,
            note: n,
            created_at: c,
        }))
    }

    async fn list_by_owner(
        &self,
        owner_handle: &str,
    ) -> Result<Vec<ShareMeta>, ShareMetaError> {
        let rows = sqlx::query_as::<
            _,
            (String, String, Option<DateTime<Utc>>, Option<String>, DateTime<Utc>),
        >(
            r#"
            SELECT owner_handle, recipient_handle, expires_at, note, created_at
            FROM share_metadata
            WHERE lower(owner_handle) = lower($1)
            "#,
        )
        .bind(owner_handle)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(o, r, e, n, c)| ShareMeta {
                owner_handle: o,
                recipient_handle: r,
                expires_at: e,
                note: n,
                created_at: c,
            })
            .collect())
    }

    async fn list_by_recipient(
        &self,
        recipient_handle: &str,
    ) -> Result<Vec<ShareMeta>, ShareMetaError> {
        let rows = sqlx::query_as::<
            _,
            (String, String, Option<DateTime<Utc>>, Option<String>, DateTime<Utc>),
        >(
            r#"
            SELECT owner_handle, recipient_handle, expires_at, note, created_at
            FROM share_metadata
            WHERE lower(recipient_handle) = lower($1)
            "#,
        )
        .bind(recipient_handle)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(o, r, e, n, c)| ShareMeta {
                owner_handle: o,
                recipient_handle: r,
                expires_at: e,
                note: n,
                created_at: c,
            })
            .collect())
    }

    async fn delete(
        &self,
        owner_handle: &str,
        recipient_handle: &str,
    ) -> Result<(), ShareMetaError> {
        sqlx::query(
            r#"
            DELETE FROM share_metadata
            WHERE lower(owner_handle) = lower($1)
              AND lower(recipient_handle) = lower($2)
            "#,
        )
        .bind(owner_handle)
        .bind(recipient_handle)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory store used by the sharing-route tests. Key shape
    /// mirrors the Postgres PK: lower-cased handle pair.
    pub struct MemoryShareMetadataStore {
        rows: Mutex<HashMap<(String, String), ShareMeta>>,
    }

    impl Default for MemoryShareMetadataStore {
        fn default() -> Self {
            Self {
                rows: Mutex::new(HashMap::new()),
            }
        }
    }

    fn key(owner: &str, recipient: &str) -> (String, String) {
        (owner.to_ascii_lowercase(), recipient.to_ascii_lowercase())
    }

    #[async_trait]
    impl ShareMetadataStore for MemoryShareMetadataStore {
        async fn upsert(
            &self,
            owner_handle: &str,
            recipient_handle: &str,
            expires_at: Option<DateTime<Utc>>,
            note: Option<&str>,
        ) -> Result<(), ShareMetaError> {
            let mut rows = self.rows.lock().unwrap();
            rows.insert(
                key(owner_handle, recipient_handle),
                ShareMeta {
                    owner_handle: owner_handle.to_string(),
                    recipient_handle: recipient_handle.to_string(),
                    expires_at,
                    note: note.map(|s| s.to_string()),
                    created_at: Utc::now(),
                },
            );
            Ok(())
        }

        async fn find(
            &self,
            owner_handle: &str,
            recipient_handle: &str,
        ) -> Result<Option<ShareMeta>, ShareMetaError> {
            let rows = self.rows.lock().unwrap();
            Ok(rows.get(&key(owner_handle, recipient_handle)).cloned())
        }

        async fn list_by_owner(
            &self,
            owner_handle: &str,
        ) -> Result<Vec<ShareMeta>, ShareMetaError> {
            let owner_lower = owner_handle.to_ascii_lowercase();
            let rows = self.rows.lock().unwrap();
            Ok(rows
                .iter()
                .filter(|((o, _), _)| o == &owner_lower)
                .map(|(_, m)| m.clone())
                .collect())
        }

        async fn list_by_recipient(
            &self,
            recipient_handle: &str,
        ) -> Result<Vec<ShareMeta>, ShareMetaError> {
            let rec_lower = recipient_handle.to_ascii_lowercase();
            let rows = self.rows.lock().unwrap();
            Ok(rows
                .iter()
                .filter(|((_, r), _)| r == &rec_lower)
                .map(|(_, m)| m.clone())
                .collect())
        }

        async fn delete(
            &self,
            owner_handle: &str,
            recipient_handle: &str,
        ) -> Result<(), ShareMetaError> {
            let mut rows = self.rows.lock().unwrap();
            rows.remove(&key(owner_handle, recipient_handle));
            Ok(())
        }
    }
}
