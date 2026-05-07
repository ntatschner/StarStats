//! Hash-chained audit log writer.
//!
//! Every API call that changes server state appends one row. The
//! database trigger (`audit_log_check_chain`) verifies the chain on
//! insert; the application computes the hash before sending the
//! INSERT, and uses a transaction with `SELECT ... FOR UPDATE` on the
//! tail row to serialise concurrent writers.
//!
//! Hash construction:
//!   prev_hash || canonical(payload) || seq.to_string()
//! Canonical JSON has a fixed key order so two equal logical payloads
//! always produce the same digest.
//!
//! ## MinIO mirror
//! After the Postgres INSERT commits, the same row is replicated to
//! the configured S3-compatible bucket via [`MinioMirror`]. Mirror
//! failures are logged at `warn` and do **not** roll back or retry —
//! Postgres remains the system of record (see `docs/AUDIT.md`
//! "Mirroring").

use crate::audit_mirror::{AuditEntryRow, MinioMirror};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("payload serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub actor_sub: Option<String>,
    pub actor_handle: Option<String>,
    pub action: String,
    pub payload: Value,
}

#[async_trait]
pub trait AuditLog: Send + Sync + 'static {
    async fn append(&self, entry: AuditEntry) -> Result<(), AuditError>;
}

pub struct PostgresAuditLog {
    pool: PgPool,
    /// Optional MinIO/S3 mirror. `None` = no mirror configured (local
    /// dev or skipped homelab dep). Wrapped in `Arc` so this struct
    /// stays cheap to share across handlers.
    mirror: Option<Arc<MinioMirror>>,
}

impl PostgresAuditLog {
    /// Construct without a mirror. The Postgres write remains the
    /// source of truth; no S3 PUTs are issued.
    pub fn new(pool: PgPool) -> Self {
        Self { pool, mirror: None }
    }

    /// Builder-style attachment of a MinIO/S3 mirror. Pass `None` to
    /// keep the mirror disabled (useful for tests). When `Some`, every
    /// successful Postgres INSERT is followed by a best-effort PUT to
    /// the configured bucket.
    pub fn with_mirror(mut self, mirror: Option<Arc<MinioMirror>>) -> Self {
        self.mirror = mirror;
        self
    }
}

#[async_trait]
impl AuditLog for PostgresAuditLog {
    async fn append(&self, entry: AuditEntry) -> Result<(), AuditError> {
        let canonical = canonicalize(&entry.payload)?;
        let mut tx = self.pool.begin().await?;

        // Lock the tail row so concurrent appenders compute the chain
        // off the same prev_hash.
        let row: Option<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT seq, row_hash FROM audit_log
             ORDER BY seq DESC LIMIT 1
             FOR UPDATE",
        )
        .fetch_optional(&mut *tx)
        .await?;

        let (next_seq, prev_hash) = match row {
            Some((seq, hash)) => (seq + 1, hash),
            None => (1i64, vec![0u8; 32]),
        };

        let mut hasher = Sha256::new();
        hasher.update(&prev_hash);
        hasher.update(&canonical);
        hasher.update(next_seq.to_string().as_bytes());
        let row_hash: [u8; 32] = hasher.finalize().into();

        // Capture the wall-clock time we send to Postgres so the
        // mirror object can be partitioned by the same timestamp the
        // DB row will record. The DB column also defaults to NOW();
        // these will be within microseconds of each other on the
        // homelab, which is fine for partition-by-day.
        let occurred_at: DateTime<Utc> = Utc::now();

        sqlx::query(
            "INSERT INTO audit_log
                (occurred_at, actor_sub, actor_handle, action, payload, prev_hash, row_hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(occurred_at)
        .bind(&entry.actor_sub)
        .bind(&entry.actor_handle)
        .bind(&entry.action)
        .bind(&entry.payload)
        .bind(&prev_hash)
        .bind(row_hash.as_slice())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Best-effort mirror write. A failure here is logged but does
        // NOT roll back the Postgres row — Postgres is the source of
        // truth, and a separate reconciliation job (out of scope for
        // this slice) handles drift detection.
        if let Some(mirror) = &self.mirror {
            let row = AuditEntryRow {
                seq: next_seq,
                occurred_at,
                actor_sub: entry.actor_sub.clone(),
                actor_handle: entry.actor_handle.clone(),
                action: entry.action.clone(),
                payload: entry.payload.clone(),
                prev_hash_hex: hex::encode(&prev_hash),
                row_hash_hex: hex::encode(row_hash),
            };
            if let Err(e) = mirror.append(&row).await {
                tracing::warn!(
                    error = %e,
                    seq = next_seq,
                    action = %entry.action,
                    "MinIO audit mirror write failed; Postgres row is authoritative"
                );
            }
        }

        Ok(())
    }
}

/// Build canonical bytes for a JSON value: object keys in
/// lexicographic order, no whitespace. `serde_json::to_vec` already
/// produces no whitespace; we reach into the value to sort keys.
fn canonicalize(v: &Value) -> Result<Vec<u8>, serde_json::Error> {
    let sorted = sort_keys(v);
    serde_json::to_vec(&sorted)
}

fn sort_keys(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> =
                map.iter().map(|(k, v)| (k.clone(), sort_keys(v))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut sorted = serde_json::Map::with_capacity(entries.len());
            for (k, v) in entries {
                sorted.insert(k, v);
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_keys).collect()),
        other => other.clone(),
    }
}

// -- Test-only in-memory log -----------------------------------------

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::sync::Mutex;

    pub struct MemoryAuditLog {
        entries: Mutex<Vec<AuditEntry>>,
    }

    impl Default for MemoryAuditLog {
        fn default() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }
    }

    impl MemoryAuditLog {
        pub fn snapshot(&self) -> Vec<AuditEntry> {
            self.entries.lock().expect("audit memlog poisoned").clone()
        }
    }

    #[async_trait]
    impl AuditLog for MemoryAuditLog {
        async fn append(&self, entry: AuditEntry) -> Result<(), AuditError> {
            self.entries
                .lock()
                .expect("audit memlog poisoned")
                .push(entry);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonicalize_orders_keys_deterministically() {
        let a = json!({ "z": 1, "a": 2, "m": { "y": 3, "x": 4 } });
        let b = json!({ "m": { "x": 4, "y": 3 }, "a": 2, "z": 1 });
        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn canonicalize_preserves_array_order() {
        let a = json!([3, 1, 2]);
        let b = json!([1, 2, 3]);
        assert_ne!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    /// Constructing `PostgresAuditLog` without a mirror must keep the
    /// mirror field as `None` so `append` skips the S3 PUT path
    /// entirely. We can't drive `append` here without a live Postgres,
    /// but we can prove the struct contract: builder default is no
    /// mirror, and `with_mirror(None)` is a no-op.
    ///
    /// `connect_lazy` requires a Tokio context (sqlx spawns a
    /// background reaper task), so the test runs under `#[tokio::test]`.
    #[tokio::test]
    async fn with_mirror_none_keeps_mirror_disabled() {
        // Use a lazy pool — we only inspect the struct, never query.
        // `connect_lazy` does not touch the network until the first
        // query, which we never issue.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://does-not-resolve/none")
            .expect("lazy pool builds");

        let log = PostgresAuditLog::new(pool.clone());
        assert!(log.mirror.is_none(), "default constructor: no mirror");

        let log = PostgresAuditLog::new(pool).with_mirror(None);
        assert!(log.mirror.is_none(), "with_mirror(None): still no mirror");
    }
}
