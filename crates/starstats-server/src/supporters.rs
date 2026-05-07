//! Supporter (donate) status data layer.
//!
//! See `docs/REVOLUT-INTEGRATION-PLAN.md` for the full lifecycle.
//! This module owns the read side and one mutation (set name_plate);
//! payment-flow mutations (state transitions on webhook) land in the
//! Wave 9 follow-up that wires up Revolut.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Hard cap on the display name string. The design spec is 28 chars;
/// we enforce here so a future API change can't accidentally relax it
/// without an explicit override. Read-side endpoint doesn't reference
/// this; the cap kicks in on the PUT endpoint that ships in the
/// Wave 9 follow-up (see `docs/REVOLUT-INTEGRATION-PLAN.md`).
#[allow(dead_code)]
pub const NAME_PLATE_MAX_CHARS: usize = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupporterState {
    None,
    Active,
    Lapsed,
}

impl SupporterState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Active => "active",
            Self::Lapsed => "lapsed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "none" => Self::None,
            "active" => Self::Active,
            "lapsed" => Self::Lapsed,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SupporterStatus {
    /// The owning user — populated for symmetry with other rows but
    /// not surfaced by the current read DTO (the caller's identity is
    /// already in the bearer token).
    #[allow(dead_code)]
    pub user_id: Uuid,
    pub state: SupporterState,
    pub name_plate: Option<String>,
    pub became_supporter_at: Option<DateTime<Utc>>,
    pub last_payment_at: Option<DateTime<Utc>>,
    pub grace_until: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    /// Last write timestamp; surfaced by the eventual webhook handler
    /// for stale-row checks, not by the read DTO.
    #[allow(dead_code)]
    pub updated_at: DateTime<Utc>,
}

impl SupporterStatus {
    /// Default surface for users with no row yet. Saves the read path
    /// from a special "no row" branch — every authenticated user can
    /// at least say "none + no plate".
    pub fn empty(user_id: Uuid) -> Self {
        Self {
            user_id,
            state: SupporterState::None,
            name_plate: None,
            became_supporter_at: None,
            last_payment_at: None,
            grace_until: None,
            cancelled_at: None,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SupporterError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[async_trait]
pub trait SupporterStore: Send + Sync + 'static {
    /// Returns the row for `user_id` or [`SupporterStatus::empty`] if
    /// no row exists yet. Never returns `None`; the caller's mental
    /// model is "every user has a status, the default is none".
    async fn get(&self, user_id: Uuid) -> Result<SupporterStatus, SupporterError>;

    /// Flip the user's state to `active` and record a payment. The
    /// webhook handler calls this when an `ORDER_COMPLETED` event
    /// lands. Idempotent: replaying the same payment is a no-op
    /// because the `revolut_webhook_events` PK fences duplicate
    /// webhook deliveries before this method is reached.
    ///
    /// `name_plate`, when `Some`, sets/replaces the existing plate.
    /// `None` leaves any existing plate untouched (don't overwrite a
    /// plate the user set later via the edit endpoint with a stale
    /// snapshot from an older order).
    async fn mark_payment_received(
        &self,
        user_id: Uuid,
        name_plate: Option<&str>,
        coverage_until: DateTime<Utc>,
    ) -> Result<(), SupporterError>;
}

pub struct PostgresSupporterStore {
    pool: PgPool,
}

impl PostgresSupporterStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SupporterStore for PostgresSupporterStore {
    async fn get(&self, user_id: Uuid) -> Result<SupporterStatus, SupporterError> {
        let row: Option<(
            String,
            Option<String>,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT state, name_plate,
                    became_supporter_at, last_payment_at,
                    grace_until, cancelled_at, updated_at
             FROM supporter_status
             WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match row {
            None => SupporterStatus::empty(user_id),
            Some((state, name_plate, became, last_pay, grace, cancelled, updated)) => {
                SupporterStatus {
                    user_id,
                    state: SupporterState::parse(&state).unwrap_or(SupporterState::None),
                    name_plate,
                    became_supporter_at: became,
                    last_payment_at: last_pay,
                    grace_until: grace,
                    cancelled_at: cancelled,
                    updated_at: updated,
                }
            }
        })
    }

    async fn mark_payment_received(
        &self,
        user_id: Uuid,
        name_plate: Option<&str>,
        coverage_until: DateTime<Utc>,
    ) -> Result<(), SupporterError> {
        // Single UPSERT: insert the row if missing, otherwise advance
        // its state. `became_supporter_at` is set on first payment
        // only (COALESCE keeps the original value on subsequent
        // payments). `name_plate` only overwrites when the caller
        // supplied one, so a later user-driven plate edit isn't
        // clobbered by a future payment that didn't carry a plate.
        sqlx::query(
            "INSERT INTO supporter_status
                (user_id, state, name_plate, became_supporter_at,
                 last_payment_at, grace_until, cancelled_at, updated_at)
             VALUES ($1, 'active', $2, NOW(), NOW(), $3, NULL, NOW())
             ON CONFLICT (user_id) DO UPDATE SET
                state = 'active',
                name_plate = COALESCE($2, supporter_status.name_plate),
                became_supporter_at =
                    COALESCE(supporter_status.became_supporter_at, NOW()),
                last_payment_at = NOW(),
                grace_until = $3,
                cancelled_at = NULL,
                updated_at = NOW()",
        )
        .bind(user_id)
        .bind(name_plate)
        .bind(coverage_until)
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

    #[derive(Default)]
    pub struct MemorySupporterStore {
        rows: Mutex<HashMap<Uuid, SupporterStatus>>,
    }

    impl MemorySupporterStore {
        pub fn seed(&self, status: SupporterStatus) {
            self.rows
                .lock()
                .expect("supporter memstore poisoned")
                .insert(status.user_id, status);
        }
    }

    #[async_trait]
    impl SupporterStore for MemorySupporterStore {
        async fn get(&self, user_id: Uuid) -> Result<SupporterStatus, SupporterError> {
            let rows = self.rows.lock().expect("supporter memstore poisoned");
            Ok(rows
                .get(&user_id)
                .cloned()
                .unwrap_or_else(|| SupporterStatus::empty(user_id)))
        }

        async fn mark_payment_received(
            &self,
            user_id: Uuid,
            name_plate: Option<&str>,
            coverage_until: DateTime<Utc>,
        ) -> Result<(), SupporterError> {
            let mut rows = self.rows.lock().expect("supporter memstore poisoned");
            let now = Utc::now();
            let entry = rows.entry(user_id).or_insert_with(|| SupporterStatus {
                user_id,
                state: SupporterState::None,
                name_plate: None,
                became_supporter_at: None,
                last_payment_at: None,
                grace_until: None,
                cancelled_at: None,
                updated_at: now,
            });
            entry.state = SupporterState::Active;
            if let Some(plate) = name_plate {
                entry.name_plate = Some(plate.to_string());
            }
            if entry.became_supporter_at.is_none() {
                entry.became_supporter_at = Some(now);
            }
            entry.last_payment_at = Some(now);
            entry.grace_until = Some(coverage_until);
            entry.cancelled_at = None;
            entry.updated_at = now;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MemorySupporterStore;
    use super::*;

    #[tokio::test]
    async fn get_returns_none_state_for_unseeded_user() {
        let store = MemorySupporterStore::default();
        let user_id = Uuid::now_v7();
        let s = store.get(user_id).await.expect("get");
        assert_eq!(s.state, SupporterState::None);
        assert!(s.name_plate.is_none());
    }

    #[tokio::test]
    async fn mark_payment_received_creates_active_row() {
        let store = MemorySupporterStore::default();
        let user_id = Uuid::now_v7();
        let coverage = Utc::now() + chrono::Duration::days(30);
        store
            .mark_payment_received(user_id, Some("Caelum"), coverage)
            .await
            .expect("mark");
        let s = store.get(user_id).await.unwrap();
        assert_eq!(s.state, SupporterState::Active);
        assert_eq!(s.name_plate.as_deref(), Some("Caelum"));
        assert!(s.became_supporter_at.is_some());
        assert!(s.last_payment_at.is_some());
        assert_eq!(s.grace_until, Some(coverage));
    }

    #[tokio::test]
    async fn mark_payment_received_preserves_existing_plate_when_none_passed() {
        let store = MemorySupporterStore::default();
        let user_id = Uuid::now_v7();
        let coverage = Utc::now() + chrono::Duration::days(30);
        store
            .mark_payment_received(user_id, Some("FirstPlate"), coverage)
            .await
            .unwrap();
        // Second payment without plate: keep the first plate.
        store
            .mark_payment_received(user_id, None, coverage + chrono::Duration::days(30))
            .await
            .unwrap();
        let s = store.get(user_id).await.unwrap();
        assert_eq!(s.name_plate.as_deref(), Some("FirstPlate"));
    }

    #[tokio::test]
    async fn get_returns_seeded_row() {
        let store = MemorySupporterStore::default();
        let user_id = Uuid::now_v7();
        store.seed(SupporterStatus {
            user_id,
            state: SupporterState::Active,
            name_plate: Some("Caelum".into()),
            became_supporter_at: Some(Utc::now()),
            last_payment_at: Some(Utc::now()),
            grace_until: None,
            cancelled_at: None,
            updated_at: Utc::now(),
        });
        let s = store.get(user_id).await.unwrap();
        assert_eq!(s.state, SupporterState::Active);
        assert_eq!(s.name_plate.as_deref(), Some("Caelum"));
    }
}
