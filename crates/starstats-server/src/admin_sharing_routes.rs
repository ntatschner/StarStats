//! Admin sharing-overview HTTP handlers.
//!
//! Two read-only endpoints surfaced on the `/admin/sharing` page,
//! both gated on the moderator role (admins inherit via
//! [`RequireModerator`]):
//!
//!   - `GET /v1/admin/sharing/overview`        — headline counters +
//!     time-windowed totals + top-20 granters.
//!   - `GET /v1/admin/sharing/scope-histogram` — distribution of
//!     `share_metadata.scope->>'kind'` across active shares plus a
//!     per-tab usage breakdown for `kind = 'tabs'` rows.
//!
//! The overview replaces a frontend proxy that used the recent
//! audit-log window — that proxy could see at most N events, so the
//! "active shares" card lied as soon as activity exceeded N. These
//! endpoints aggregate the real `share_metadata` table so the cards
//! match ground truth, with `audit_log` only consulted for the
//! genuinely time-windowed counters (30-day grants / revokes / views).

use crate::admin_routes::RequireModerator;
use crate::audit::{AuditFilters, AuditQuery};
use crate::share_metadata::ShareMetadataStore;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Extension, Router,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use utoipa::ToSchema;

// -- DTOs ------------------------------------------------------------

/// One row of the "top granters" leaderboard. Mirrors
/// [`crate::share_metadata::GranterCount`] but typed for the wire
/// (counts narrowed to u64 since they're always non-negative).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TopGranter {
    /// Owner handle as stored on the underlying row (case-preserved
    /// display copy). Suitable for direct rendering in the UI.
    pub handle: String,
    /// Number of `share_metadata` rows owned by this handle that are
    /// still active (no expiry, or expiry in the future).
    pub active_share_count: u64,
}

/// Response body for `GET /v1/admin/sharing/overview`. The
/// `active_shares_*` counters come from a snapshot of `share_metadata`;
/// the `total_*_30d` counters come from a `WHERE occurred_at >
/// NOW() - INTERVAL '30 days'` filter on `audit_log`. Both shapes are
/// flattened into one DTO because the admin UI renders them as a
/// single card row — splitting the response into two would force the
/// page to make two round-trips for one render.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminSharingOverview {
    /// COUNT(*) from `share_metadata` WHERE `expires_at IS NULL OR
    /// expires_at > NOW()`. The headline "live shares" number.
    pub active_shares_total: u64,
    /// User-to-user (outbound) active shares. `share_metadata` only
    /// stores user-to-user shares today; org-direction grants live
    /// in SpiceDB without a side-table row, so this matches `_total`
    /// until the org table lands.
    pub active_shares_outbound: u64,
    /// Subset of `_total` where the share has an explicit expiry in
    /// the future (i.e. excludes legacy "no expiry" rows).
    pub active_shares_with_expiry: u64,
    /// Top owner handles by active-share count, sorted DESC, capped
    /// at 20. Empty when the table is empty.
    pub top_granters: Vec<TopGranter>,
    /// `share.created` audit rows in the last 30 days. Time-windowed
    /// rather than absolute — the audit log doesn't keep "current
    /// state".
    pub total_grants_30d: u64,
    /// `share.revoked` audit rows in the last 30 days.
    pub total_revocations_30d: u64,
    /// `share.viewed` audit rows in the last 30 days.
    pub total_views_30d: u64,
}

/// Response body for `GET /v1/admin/sharing/scope-histogram`. NULL
/// scope rows fold into `full` (the legacy default preserved by
/// migration 0025). `tab_usage` is per-tab counts for `kind = 'tabs'`
/// rows; map keys are stable (BTreeMap) so the wire payload is
/// reproducible across requests with the same data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ScopeHistogram {
    pub full: u64,
    pub timeline: u64,
    pub aggregates: u64,
    pub tabs: u64,
    /// Per-tab usage on `kind = 'tabs'` rows. Always emitted (empty
    /// `{}` when no tabs-scope rows exist) so the UI can render the
    /// card without a presence check.
    pub tab_usage: BTreeMap<String, u64>,
}

// -- Handlers --------------------------------------------------------

/// Cap on `top_granters`. Twenty is large enough to cover an entire
/// power-user cohort while keeping the JSON payload trivially small.
const TOP_GRANTERS_LIMIT: i64 = 20;

/// GET /v1/admin/sharing/overview — replaces the audit-window proxy
/// that the admin UI used before this endpoint existed. Gated on
/// moderator; admins inherit.
#[utoipa::path(
    get,
    path = "/v1/admin/sharing/overview",
    tag = "admin",
    responses(
        (status = 200, description = "Sharing overview snapshot", body = AdminSharingOverview),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks moderator role"),
        (status = 500, description = "Database error"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn get_overview(
    _: RequireModerator,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
    Extension(audit_query): Extension<Arc<dyn AuditQuery>>,
) -> Response {
    // Active-share counters off the `share_metadata` table.
    let counts = match meta.active_share_counts().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "share_metadata active_share_counts failed");
            return overview_500();
        }
    };
    let top = match meta.top_active_granters(TOP_GRANTERS_LIMIT).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "share_metadata top_active_granters failed");
            return overview_500();
        }
    };

    // 30-day audit-log counters. We reuse `AuditQuery::list` with the
    // `since` filter plus a tight `limit`; the page size is just an
    // upper bound on what we'll read, since we only consume the
    // returned length. 500 is the store's max page size — exceeding
    // it silently clamps, which would understate the counter. For the
    // homelab data volume this is fine; a follow-up wave can add a
    // dedicated COUNT(*) when audit grows past the cap.
    let since = Utc::now() - Duration::days(30);
    let grants_30d = match count_audit_action(&*audit_query, "share.created", since).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(error = %e, "audit count (share.created) failed");
            return overview_500();
        }
    };
    let revokes_30d = match count_audit_action(&*audit_query, "share.revoked", since).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(error = %e, "audit count (share.revoked) failed");
            return overview_500();
        }
    };
    let views_30d = match count_audit_action(&*audit_query, "share.viewed", since).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(error = %e, "audit count (share.viewed) failed");
            return overview_500();
        }
    };

    let body = AdminSharingOverview {
        active_shares_total: counts.total.max(0) as u64,
        // share_metadata only stores user→user (outbound) rows today;
        // see DTO doc for the org caveat.
        active_shares_outbound: counts.total.max(0) as u64,
        active_shares_with_expiry: counts.with_expiry.max(0) as u64,
        top_granters: top
            .into_iter()
            .map(|g| TopGranter {
                handle: g.handle,
                active_share_count: g.active_share_count.max(0) as u64,
            })
            .collect(),
        total_grants_30d: grants_30d,
        total_revocations_30d: revokes_30d,
        total_views_30d: views_30d,
    };
    (StatusCode::OK, Json(body)).into_response()
}

/// GET /v1/admin/sharing/scope-histogram — distribution of scope
/// kinds across active shares. Gated on moderator.
#[utoipa::path(
    get,
    path = "/v1/admin/sharing/scope-histogram",
    tag = "admin",
    responses(
        (status = 200, description = "Scope-kind distribution across active shares", body = ScopeHistogram),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks moderator role"),
        (status = 500, description = "Database error"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn get_scope_histogram(
    _: RequireModerator,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
) -> Response {
    let h = match meta.scope_histogram_active().await {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "share_metadata scope_histogram_active failed");
            return overview_500();
        }
    };
    let body = ScopeHistogram {
        full: h.full.max(0) as u64,
        timeline: h.timeline.max(0) as u64,
        aggregates: h.aggregates.max(0) as u64,
        tabs: h.tabs.max(0) as u64,
        tab_usage: h
            .tab_usage
            .into_iter()
            .map(|(k, v)| (k, v.max(0) as u64))
            .collect(),
    };
    (StatusCode::OK, Json(body)).into_response()
}

// -- Helpers ---------------------------------------------------------

/// 500 envelope used by both handlers — kept local because the
/// existing `api_error::ApiErrorBody` is overkill for these
/// admin-only routes where the UI never parses the failure body.
fn overview_500() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "sharing_overview_failed"})),
    )
        .into_response()
}

/// Run a single `AuditQuery::list` for one action over the given
/// window. Returns the row count.
///
/// We don't have a dedicated COUNT method on `AuditQuery` yet — adding
/// one would touch every implementer, and the homelab volume comfortably
/// fits in the 500-row page cap. The cap is documented at the call
/// site.
async fn count_audit_action(
    audit: &dyn AuditQuery,
    action: &str,
    since: chrono::DateTime<Utc>,
) -> Result<u64, crate::audit::AuditError> {
    let rows = audit
        .list(AuditFilters {
            actor_handle: None,
            action: Some(action.to_string()),
            since: Some(since),
            until: None,
            limit: 500,
            offset: 0,
        })
        .await?;
    Ok(rows.len() as u64)
}

/// Build the sharing-admin sub-router. Parameterless: both handlers
/// pull their stores off `Extension` layers installed on the outer
/// router (`Arc<dyn ShareMetadataStore>` + `Arc<dyn AuditQuery>` are
/// already provided by main.rs for the existing sharing routes and
/// the /v1/admin/audit endpoint respectively).
pub fn router() -> Router {
    Router::new()
        .route("/v1/admin/sharing/overview", get(get_overview))
        .route(
            "/v1/admin/sharing/scope-histogram",
            get(get_scope_histogram),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::test_support::MemoryAuditLog;
    use crate::audit::{AuditEntry, AuditLog};
    use crate::auth::test_support::fresh_pair;
    use crate::auth::{AuthVerifier, TokenIssuer};
    use crate::share_metadata::test_support::MemoryShareMetadataStore;
    use crate::staff_roles::test_support::MemoryStaffRoleStore;
    use crate::staff_roles::{StaffRole, StaffRoleStore};
    use axum::body::to_bytes;
    use axum::http::Request;
    use chrono::Duration as ChronoDuration;
    use serde_json::json;
    use tower::ServiceExt;
    use uuid::Uuid;

    /// Wire the sharing-admin router with the full extension stack.
    fn build_app(
        meta: Arc<MemoryShareMetadataStore>,
        audit: Arc<MemoryAuditLog>,
        staff: Arc<MemoryStaffRoleStore>,
        verifier: Arc<AuthVerifier>,
    ) -> Router {
        let meta_dyn: Arc<dyn ShareMetadataStore> = meta;
        let audit_dyn: Arc<dyn AuditQuery> = audit;
        let staff_dyn: Arc<dyn StaffRoleStore> = staff;
        router()
            .layer(Extension(verifier))
            .layer(Extension(meta_dyn))
            .layer(Extension(audit_dyn))
            .layer(Extension(staff_dyn))
    }

    async fn json_body<T: for<'de> serde::Deserialize<'de>>(resp: Response) -> T {
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            panic!(
                "decode {}: {} (body={})",
                std::any::type_name::<T>(),
                e,
                String::from_utf8_lossy(&bytes)
            )
        })
    }

    /// Build a moderator-roled user and return its bearer token.
    async fn moderator_token(
        staff: &MemoryStaffRoleStore,
        issuer: &TokenIssuer,
        handle: &str,
    ) -> String {
        let user_id = Uuid::now_v7();
        staff
            .grant(user_id, StaffRole::Moderator, None, None)
            .await
            .unwrap();
        issuer
            .sign_user(&user_id.to_string(), handle)
            .expect("sign moderator token")
    }

    /// Bearer for a regular (non-staff) user.
    fn plain_token(issuer: &TokenIssuer, handle: &str) -> String {
        issuer
            .sign_user(&Uuid::now_v7().to_string(), handle)
            .expect("sign plain token")
    }

    fn get(uri: &str, token: &str) -> Request<axum::body::Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap()
    }

    // -- /overview ---------------------------------------------------

    #[tokio::test]
    async fn overview_empty_returns_zeroes_for_moderator() {
        let meta = Arc::new(MemoryShareMetadataStore::default());
        let audit = Arc::new(MemoryAuditLog::default());
        let staff = Arc::new(MemoryStaffRoleStore::new());
        let (issuer, verifier) = fresh_pair();
        let app = build_app(meta, audit, staff.clone(), Arc::new(verifier));
        let tok = moderator_token(&staff, &issuer, "mod").await;

        let resp = app
            .oneshot(get("/v1/admin/sharing/overview", &tok))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: AdminSharingOverview = json_body(resp).await;
        assert_eq!(body.active_shares_total, 0);
        assert_eq!(body.active_shares_outbound, 0);
        assert_eq!(body.active_shares_with_expiry, 0);
        assert!(body.top_granters.is_empty());
        assert_eq!(body.total_grants_30d, 0);
        assert_eq!(body.total_revocations_30d, 0);
        assert_eq!(body.total_views_30d, 0);
    }

    #[tokio::test]
    async fn overview_rejects_non_staff_with_403() {
        let meta = Arc::new(MemoryShareMetadataStore::default());
        let audit = Arc::new(MemoryAuditLog::default());
        let staff = Arc::new(MemoryStaffRoleStore::new());
        let (issuer, verifier) = fresh_pair();
        let app = build_app(meta, audit, staff, Arc::new(verifier));
        let tok = plain_token(&issuer, "rando");

        let resp = app
            .oneshot(get("/v1/admin/sharing/overview", &tok))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn overview_counts_active_shares_top_granters_and_audit_window() {
        let meta = Arc::new(MemoryShareMetadataStore::default());
        let audit = Arc::new(MemoryAuditLog::default());
        let staff = Arc::new(MemoryStaffRoleStore::new());
        let (issuer, verifier) = fresh_pair();

        // Seed share_metadata: alice owns 3 active (1 with expiry),
        // bob owns 1 expired (should not count), carol owns 2 active
        // (no expiry).
        let future = Utc::now() + ChronoDuration::days(7);
        let past = Utc::now() - ChronoDuration::days(1);
        meta.upsert("alice", "r1", None, None, None).await.unwrap();
        meta.upsert("alice", "r2", None, None, None).await.unwrap();
        meta.upsert("alice", "r3", Some(future), None, None)
            .await
            .unwrap();
        meta.upsert("bob", "r4", Some(past), None, None)
            .await
            .unwrap();
        meta.upsert("carol", "r5", None, None, None).await.unwrap();
        meta.upsert("carol", "r6", None, None, None).await.unwrap();

        // Seed audit entries in the last 30d.
        for _ in 0..4 {
            audit
                .append(AuditEntry {
                    actor_sub: None,
                    actor_handle: Some("alice".into()),
                    action: "share.created".into(),
                    payload: json!({}),
                })
                .await
                .unwrap();
        }
        for _ in 0..2 {
            audit
                .append(AuditEntry {
                    actor_sub: None,
                    actor_handle: Some("alice".into()),
                    action: "share.revoked".into(),
                    payload: json!({}),
                })
                .await
                .unwrap();
        }
        for _ in 0..7 {
            audit
                .append(AuditEntry {
                    actor_sub: None,
                    actor_handle: Some("recipient".into()),
                    action: "share.viewed".into(),
                    payload: json!({}),
                })
                .await
                .unwrap();
        }
        // Noise: an unrelated action that must NOT be counted.
        audit
            .append(AuditEntry {
                actor_sub: None,
                actor_handle: Some("alice".into()),
                action: "share.visibility_changed".into(),
                payload: json!({}),
            })
            .await
            .unwrap();

        let app = build_app(meta, audit, staff.clone(), Arc::new(verifier));
        let tok = moderator_token(&staff, &issuer, "mod").await;
        let resp = app
            .oneshot(get("/v1/admin/sharing/overview", &tok))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: AdminSharingOverview = json_body(resp).await;
        assert_eq!(body.active_shares_total, 5);
        assert_eq!(body.active_shares_outbound, 5);
        assert_eq!(body.active_shares_with_expiry, 1);
        assert_eq!(body.top_granters.len(), 2);
        assert_eq!(body.top_granters[0].handle, "alice");
        assert_eq!(body.top_granters[0].active_share_count, 3);
        assert_eq!(body.top_granters[1].handle, "carol");
        assert_eq!(body.top_granters[1].active_share_count, 2);
        assert_eq!(body.total_grants_30d, 4);
        assert_eq!(body.total_revocations_30d, 2);
        assert_eq!(body.total_views_30d, 7);
    }

    // -- /scope-histogram --------------------------------------------

    #[tokio::test]
    async fn scope_histogram_empty_returns_zeroes() {
        let meta = Arc::new(MemoryShareMetadataStore::default());
        let audit = Arc::new(MemoryAuditLog::default());
        let staff = Arc::new(MemoryStaffRoleStore::new());
        let (issuer, verifier) = fresh_pair();
        let app = build_app(meta, audit, staff.clone(), Arc::new(verifier));
        let tok = moderator_token(&staff, &issuer, "mod").await;

        let resp = app
            .oneshot(get("/v1/admin/sharing/scope-histogram", &tok))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: ScopeHistogram = json_body(resp).await;
        assert_eq!(body.full, 0);
        assert_eq!(body.timeline, 0);
        assert_eq!(body.aggregates, 0);
        assert_eq!(body.tabs, 0);
        assert!(body.tab_usage.is_empty());
    }

    #[tokio::test]
    async fn scope_histogram_rejects_non_staff_with_403() {
        let meta = Arc::new(MemoryShareMetadataStore::default());
        let audit = Arc::new(MemoryAuditLog::default());
        let staff = Arc::new(MemoryStaffRoleStore::new());
        let (issuer, verifier) = fresh_pair();
        let app = build_app(meta, audit, staff, Arc::new(verifier));
        let tok = plain_token(&issuer, "rando");

        let resp = app
            .oneshot(get("/v1/admin/sharing/scope-histogram", &tok))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn scope_histogram_tallies_kinds_and_tab_usage() {
        let meta = Arc::new(MemoryShareMetadataStore::default());
        let audit = Arc::new(MemoryAuditLog::default());
        let staff = Arc::new(MemoryStaffRoleStore::new());
        let (issuer, verifier) = fresh_pair();

        // Two NULL-scope rows (count as full), one explicit-full,
        // one timeline, one aggregates, one expired (must NOT count),
        // plus two tabs-scope rows sharing some tab names.
        meta.upsert("o1", "r1", None, None, None).await.unwrap();
        meta.upsert("o2", "r2", None, None, None).await.unwrap();
        meta.upsert("o3", "r3", None, None, Some(&json!({"kind": "full"})))
            .await
            .unwrap();
        meta.upsert(
            "o4",
            "r4",
            None,
            None,
            Some(&json!({"kind": "timeline"})),
        )
        .await
        .unwrap();
        meta.upsert(
            "o5",
            "r5",
            None,
            None,
            Some(&json!({"kind": "aggregates"})),
        )
        .await
        .unwrap();
        let past = Utc::now() - ChronoDuration::days(1);
        meta.upsert("o6", "r6", Some(past), None, Some(&json!({"kind": "full"})))
            .await
            .unwrap();
        meta.upsert(
            "o7",
            "r7",
            None,
            None,
            Some(&json!({"kind": "tabs", "tabs": ["combat", "travel"]})),
        )
        .await
        .unwrap();
        meta.upsert(
            "o8",
            "r8",
            None,
            None,
            Some(&json!({"kind": "tabs", "tabs": ["combat", "loadout"]})),
        )
        .await
        .unwrap();

        let app = build_app(meta, audit, staff.clone(), Arc::new(verifier));
        let tok = moderator_token(&staff, &issuer, "mod").await;
        let resp = app
            .oneshot(get("/v1/admin/sharing/scope-histogram", &tok))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: ScopeHistogram = json_body(resp).await;
        assert_eq!(body.full, 3, "two NULL-scope + one explicit kind=full");
        assert_eq!(body.timeline, 1);
        assert_eq!(body.aggregates, 1);
        assert_eq!(body.tabs, 2);
        assert_eq!(body.tab_usage.get("combat"), Some(&2));
        assert_eq!(body.tab_usage.get("travel"), Some(&1));
        assert_eq!(body.tab_usage.get("loadout"), Some(&1));
    }
}
