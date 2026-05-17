//! Sharing + public-visibility endpoints.
//!
//! These handlers manage the wave-2 ReBAC bits in SpiceDB:
//!  - `/v1/me/visibility` toggles the `public_view@user:*` wildcard
//!    on the caller's `stats_record`.
//!  - `/v1/me/share*` grants/revokes per-user shares
//!    (`share_with_user@user:<recipient>`).
//!  - `/v1/public/{handle}/*` exposes the summary + timeline for users
//!    who have flipped the public toggle (no auth, SpiceDB-gated).
//!  - `/v1/u/{handle}/*` does the same for authenticated callers,
//!    resolving through `share_with_user` so a recipient can read a
//!    friend's stats.
//!
//! Failure posture:
//!  - SpiceDB unavailable -> 503 `{"error":"spicedb_unavailable"}`.
//!  - Recipient unknown -> 404 (handle lookup is the only Postgres
//!    side-effect of these handlers; the rest lives in SpiceDB).
//!  - Permission denied on a public/friend read -> 404 (don't leak
//!    user existence).

use crate::api_error::ApiErrorBody;
use crate::audit::{AuditEntry, AuditLog, AuditQuery};
use crate::auth::AuthenticatedUser;
use crate::orgs::{OrgStore, PostgresOrgStore};
use crate::repo::{EventQuery, PostgresStore};
use crate::share_metadata::{ShareMetadataStore, NOTE_MAX_LEN};
use crate::share_reports::{
    rate_limit_window, ShareReportError, ShareReportReason, ShareReportStore, DETAILS_MAX_LEN,
    RATE_LIMIT_PER_WINDOW,
};
use crate::spicedb::{ObjectRef, SpicedbClient};
use crate::users::{PostgresUserStore, UserStore};
use crate::validation::{build_timeline_buckets, resolve_timeline_days};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

/// Build the `/v1/me/share*`, `/v1/me/visibility`, `/v1/public/*`,
/// and `/v1/u/*` sub-router.
///
/// Five internal sub-routers because `State<_>` shapes diverge:
///  - `add_share` needs `Arc<UserStore>` (recipient lookup).
///  - `share_with_org` needs `Arc<OrgStore>` (org existence check).
///  - public/friend reads need `Arc<EventQuery>` (the data source).
///  - the remaining toggles need no State, only Extensions.
pub fn routes(
    users: Arc<PostgresUserStore>,
    orgs: Arc<PostgresOrgStore>,
    store: Arc<PostgresStore>,
) -> Router {
    // Both `add_share` and `set_visibility` need the user store —
    // the former for recipient lookup, both for the rsi-verified gate.
    let share_user_router = Router::new()
        .route("/v1/me/share", post(add_share::<PostgresUserStore>))
        .route(
            "/v1/me/visibility",
            post(set_visibility::<PostgresUserStore>).get(get_visibility),
        )
        .with_state(users.clone());

    let share_no_state_router: Router = Router::new()
        .route("/v1/me/shares", get(list_shares))
        .route("/v1/me/shared-with-me", get(list_shared_with_me))
        .route("/v1/me/share/:recipient_handle", delete(delete_share))
        .route("/v1/share/report", post(report_share));

    let share_org_post_router = Router::new()
        .route(
            "/v1/me/share/org",
            post(share_with_org::<PostgresOrgStore, PostgresUserStore>),
        )
        .with_state((orgs, users));

    let share_org_delete_router: Router =
        Router::new().route("/v1/me/share/org/:slug", delete(unshare_with_org));

    let share_query_router = Router::new()
        .route(
            "/v1/public/:handle/summary",
            get(public_summary::<PostgresStore>),
        )
        .route(
            "/v1/public/:handle/timeline",
            get(public_timeline::<PostgresStore>),
        )
        .route(
            "/v1/u/:handle/summary",
            get(friend_summary::<PostgresStore>),
        )
        .route(
            "/v1/u/:handle/timeline",
            get(friend_timeline::<PostgresStore>),
        )
        // Audit v2.1 §B1 — owner-side preview of own data through a
        // scope clamp. No SpiceDB check, no audit emission.
        .route(
            "/v1/me/preview-share/summary",
            get(preview_summary::<PostgresStore>),
        )
        .route(
            "/v1/me/preview-share/timeline",
            get(preview_timeline::<PostgresStore>),
        )
        .with_state(store);

    share_user_router
        .merge(share_no_state_router)
        .merge(share_org_post_router)
        .merge(share_org_delete_router)
        .merge(share_query_router)
}

// -- DTOs ------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct VisibilityRequest {
    pub public: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VisibilityResponse {
    pub public: bool,
}

/// Per-share scope clamp — audit v2 §05.1+§05.5. `None` (= column
/// `NULL`) means "full manifest" which is the legacy behaviour every
/// pre-0025 share already has. Fields are intentionally permissive
/// so the front-end can ship new clamp combos without a wire break;
/// the handler validates kind + bounds.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ShareScope {
    /// One of `full`, `timeline`, `aggregates`, `tabs`. `full` = no
    /// clamp; equivalent to `scope = null` but written explicitly.
    pub kind: String,
    /// Only relevant when `kind = "tabs"` — which named profile tabs
    /// the recipient may load. Validated against the same allowlist
    /// the frontend renders so a stale client can't smuggle a tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<Vec<String>>,
    /// Clamp timeline / aggregate windows. `null` = no clamp (defer
    /// to the request's `?days=`). Capped at the same max the
    /// timeline endpoint enforces (90) so a scope can't extend access
    /// beyond what the owner could request themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_days: Option<u32>,
    /// Allowlist of event types (e.g. `quantum_target_selected`). When
    /// set, the friend summary/timeline drops any other type before
    /// returning. Mutually composable with `deny_event_types` — the
    /// allowlist is applied first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_event_types: Option<Vec<String>>,
    /// Denylist of event types (e.g. `actor_death`). When set, the
    /// friend summary/timeline drops these types from the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny_event_types: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ShareRequest {
    pub recipient_handle: String,
    /// Optional auto-expiry. ISO-8601 timestamptz. Missing or null
    /// means "share never expires" (the legacy behaviour from before
    /// metadata existed). Past timestamps are rejected at the
    /// handler with 400.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// Optional free-text note. Capped at NOTE_MAX_LEN. Empty
    /// strings collapse to `None` server-side so the column never
    /// stores `""`.
    #[serde(default)]
    pub note: Option<String>,
    /// Optional per-share scope clamp. Missing/null = "full manifest"
    /// (the pre-0025 default that every existing share already has).
    #[serde(default)]
    pub scope: Option<ShareScope>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ShareResponse {
    pub shared_with: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RevokeShareResponse {
    pub revoked: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ShareEntry {
    pub recipient_handle: String,
    /// Auto-expiry from `share_metadata`. Missing/null means
    /// "no expiry recorded" — historically all shares look like this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Owner-supplied note from `share_metadata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Per-share scope clamp. `None` (= `null` on the wire) means
    /// "full manifest", the legacy default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ShareScope>,
    /// Count of recorded `share.viewed` audit rows where this owner is
    /// the payload `owner_handle` and this recipient is the payload
    /// `recipient_handle`. Always present (zero for never-viewed) so
    /// the client can render `viewed N times` without nullish checks.
    #[serde(default)]
    pub view_count: i64,
    /// Wall-clock timestamp of the most recent `share.viewed` row, if
    /// any. Missing = never viewed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_viewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OrgShareEntry {
    pub org_slug: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListSharesResponse {
    pub shares: Vec<ShareEntry>,
    /// Orgs with `share_with_org` rows pointing at the caller's
    /// stats_record. Always present (empty array when no org shares
    /// exist) so the client can use a single property without nullish
    /// checks.
    #[serde(default)]
    pub org_shares: Vec<OrgShareEntry>,
}

/// One inbound share: an owner who has granted the caller view
/// access to their `stats_record`. The mirror of `ShareEntry`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SharedWithMeEntry {
    pub owner_handle: String,
    /// Auto-expiry as set by the owner. Surfaced so recipients know
    /// when access will lapse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Owner-supplied note explaining the grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Per-share scope clamp surfaced so the recipient understands
    /// which tabs they can actually load. Missing = full manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ShareScope>,
}

/// Response for `GET /v1/me/shared-with-me` — the inbound side of
/// per-user sharing. Org-mediated shares (org membership ->
/// `share_with_org`) are not enumerated here because they're a
/// transitive grant rather than a direct one; the org list comes
/// from `/v1/orgs/me` and the shares-to-that-org list lives under
/// org detail pages.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListSharedWithMeResponse {
    pub shared_with_me: Vec<SharedWithMeEntry>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ShareOrgRequest {
    pub org_slug: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ShareOrgResponse {
    pub shared_with_org: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RevokeOrgShareResponse {
    pub revoked: bool,
}

// Mirror of `query::SummaryResponse` so the public endpoints don't
// need to reach across modules. The OpenAPI generator emits both
// shapes; the web client treats them as compatible.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PublicSummaryResponse {
    pub claimed_handle: String,
    pub total: u64,
    pub by_type: Vec<PublicTypeCount>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PublicTypeCount {
    pub event_type: String,
    pub count: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PublicTimelineResponse {
    pub days: u32,
    pub buckets: Vec<PublicTimelineBucket>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PublicTimelineBucket {
    pub date: String,
    pub count: u64,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PublicTimelineParams {
    /// Number of trailing days to bucket. Defaults to 30, max 90.
    #[serde(default)]
    pub days: Option<u32>,
}

// -- Helpers ---------------------------------------------------------

fn err(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(ApiErrorBody {
            error: code.to_string(),
            detail: None,
        }),
    )
        .into_response()
}

fn no_store() -> [(header::HeaderName, &'static str); 1] {
    [(header::CACHE_CONTROL, "no-store")]
}

/// Allowlist of tab names the scope picker can reference. Kept in
/// lockstep with the profile-detail page tabs; smuggling an unknown
/// tab is rejected so a stale client can't lie about a "tabs" scope.
const ALLOWED_SCOPE_TABS: &[&str] = &[
    "location",
    "travel",
    "combat",
    "loadout",
    "stability",
    "commerce",
];

/// Hard cap on `scope.window_days`. Matches the timeline endpoint
/// limit so a scope can't grant access wider than the owner could
/// request themselves.
const SCOPE_MAX_WINDOW_DAYS: u32 = 90;

/// Bound the per-list event-type allow/deny vectors to keep payload
/// JSONB sizes predictable. The audit/UI surfaces today don't go
/// anywhere near this number — it's a defence-in-depth cap.
const SCOPE_MAX_TYPES: usize = 32;

/// Validate a ShareScope from a client request. Returns `Ok(())` for
/// `None` (= "full manifest") or for a well-formed scope; returns
/// `Err(error_code)` for anything we'd want the client to fix and
/// resubmit. Validation is intentionally strict on `kind` (closed
/// vocabulary) but tolerant on optional vectors (presence is enough,
/// invalid items get rejected one-by-one).
fn validate_scope(scope: &ShareScope) -> Result<(), &'static str> {
    match scope.kind.as_str() {
        "full" | "timeline" | "aggregates" | "tabs" => {}
        _ => return Err("invalid_scope_kind"),
    }
    if let Some(days) = scope.window_days {
        if days == 0 || days > SCOPE_MAX_WINDOW_DAYS {
            return Err("invalid_scope_window");
        }
    }
    if let Some(tabs) = scope.tabs.as_ref() {
        if tabs.len() > SCOPE_MAX_TYPES {
            return Err("invalid_scope_tabs");
        }
        for t in tabs {
            if !ALLOWED_SCOPE_TABS.contains(&t.as_str()) {
                return Err("invalid_scope_tabs");
            }
        }
    }
    for list in [&scope.allow_event_types, &scope.deny_event_types] {
        if let Some(types) = list.as_ref() {
            if types.len() > SCOPE_MAX_TYPES {
                return Err("invalid_scope_types");
            }
            for t in types {
                // Event types are snake_case ASCII in the parser
                // dictionary; rejecting anything else stops a typo
                // from silently passing the deny-list.
                if t.is_empty()
                    || t.len() > 64
                    || !t
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    return Err("invalid_scope_types");
                }
            }
        }
    }
    Ok(())
}

/// Pull a ShareScope back out of the stored JSONB. Returns `None` if
/// the column is null or the JSON is shaped wrong — read paths fall
/// back to "no clamp" rather than 500ing because a malformed scope
/// can't have been written through the validated `add_share` path.
fn scope_from_value(v: &serde_json::Value) -> Option<ShareScope> {
    serde_json::from_value::<ShareScope>(v.clone()).ok()
}

/// Apply scope.kind to a timeline-or-summary request. Returns
/// `Ok(())` if the read is allowed; `Err(())` if the scope's kind
/// excludes this surface (which 404s the request the same way an
/// unknown handle would).
fn scope_allows_timeline(scope: &ShareScope) -> bool {
    matches!(scope.kind.as_str(), "full" | "timeline" | "tabs")
}

fn scope_allows_aggregates(scope: &ShareScope) -> bool {
    matches!(scope.kind.as_str(), "full" | "aggregates" | "tabs")
}

/// Clamp `days` against `scope.window_days`. Returns the tighter of
/// the two; `None` window = no clamp.
fn clamp_days(days: u32, scope: Option<&ShareScope>) -> u32 {
    match scope.and_then(|s| s.window_days) {
        Some(w) => days.min(w),
        None => days,
    }
}

/// Apply event-type allow/deny filters to a `(event_type, count)`
/// list. Returns a new (total, filtered) pair. The allowlist takes
/// precedence: types absent from a non-empty allowlist are dropped
/// before the denylist is consulted.
fn apply_event_type_filter(
    rows: Vec<(String, u64)>,
    scope: Option<&ShareScope>,
) -> (u64, Vec<(String, u64)>) {
    let allow = scope.and_then(|s| s.allow_event_types.as_ref());
    let deny = scope.and_then(|s| s.deny_event_types.as_ref());
    let filtered: Vec<(String, u64)> = rows
        .into_iter()
        .filter(|(t, _)| {
            if let Some(a) = allow {
                if !a.iter().any(|x| x == t) {
                    return false;
                }
            }
            if let Some(d) = deny {
                if d.iter().any(|x| x == t) {
                    return false;
                }
            }
            true
        })
        .collect();
    let total = filtered.iter().map(|(_, c)| *c).sum();
    (total, filtered)
}

/// Best-effort audit emission for a `share.viewed` row. Logged + not
/// fatal — a hiccup in the audit pipeline must never block a friend
/// read. Owner + recipient are in the payload because the actor on
/// this audit kind is the *viewer*, not the owner.
async fn emit_share_viewed(audit: &dyn AuditLog, viewer: &AuthenticatedUser, owner: &str) {
    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(viewer.sub.clone()),
            actor_handle: Some(viewer.preferred_username.clone()),
            action: "share.viewed".to_string(),
            payload: serde_json::json!({
                "owner_handle": owner,
                "recipient_handle": viewer.preferred_username,
            }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (share.viewed)");
    }
}

fn validate_handle(handle: &str) -> bool {
    // RSI handles in our store are case-insensitive ASCII identifiers.
    // Be conservative here so a path/body argument can't be used to
    // smuggle whitespace or wildcards into a SpiceDB write. Length cap
    // matches the migration's `claimed_handle` column (varchar 64).
    !handle.is_empty()
        && handle.len() <= 64
        && handle
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Block claim-making sharing operations (public toggle, per-user
/// share, org share) until the caller has proven they own the
/// handle their account is signed up under. Returns `Some(403)`
/// when unverified — handlers `?` this at the top of the handler
/// before any SpiceDB or audit work.
///
/// Reads-only ops (`get_visibility`, `list_shares`, `delete_share`,
/// `unshare_with_org`) are deliberately NOT gated: a user must be
/// able to walk back state if a handle dispute happens after sign-up.
async fn require_rsi_verified<U: UserStore>(
    users: &U,
    auth: &AuthenticatedUser,
) -> Option<Response> {
    let sub = match uuid::Uuid::parse_str(&auth.sub) {
        Ok(id) => id,
        Err(_) => return Some(err(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject")),
    };
    match users.find_by_id(sub).await {
        Ok(Some(u)) if u.rsi_verified_at.is_some() => None,
        Ok(Some(_)) => Some(err(StatusCode::FORBIDDEN, "rsi_handle_not_verified")),
        Ok(None) => Some(err(StatusCode::UNAUTHORIZED, "unauthorized")),
        Err(e) => {
            tracing::error!(error = %e, "find_by_id failed in rsi-verify gate");
            Some(err(StatusCode::INTERNAL_SERVER_ERROR, "internal"))
        }
    }
}

// -- /v1/me/visibility -----------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/me/visibility",
    tag = "sharing",
    request_body = VisibilityRequest,
    responses(
        (status = 200, description = "Visibility toggled", body = VisibilityResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller hasn't proven RSI handle ownership", body = ApiErrorBody),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn set_visibility<U: UserStore>(
    State(users): State<Arc<U>>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    auth: AuthenticatedUser,
    Json(req): Json<VisibilityRequest>,
) -> Response {
    if let Some(resp) = require_rsi_verified(users.as_ref(), &auth).await {
        return resp;
    }

    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    let result = if req.public {
        client.write_public_view(&auth.preferred_username).await
    } else {
        client.delete_public_view(&auth.preferred_username).await
    };

    if let Err(e) = result {
        tracing::error!(error = %e, handle = %auth.preferred_username, "set visibility failed");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
    }

    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(auth.sub.clone()),
            actor_handle: Some(auth.preferred_username.clone()),
            action: "share.visibility_changed".to_string(),
            payload: serde_json::json!({ "public": req.public }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (visibility)");
    }

    (
        StatusCode::OK,
        no_store(),
        Json(VisibilityResponse { public: req.public }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/v1/me/visibility",
    tag = "sharing",
    responses(
        (status = 200, description = "Current visibility", body = VisibilityResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn get_visibility(
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    auth: AuthenticatedUser,
) -> Response {
    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    let resource = ObjectRef::new("stats_record", &auth.preferred_username);
    let subject = ObjectRef::new("user", "*");
    let public = match client.check_permission(resource, "view", subject).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "spicedb check failed (visibility get)");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
        }
    };

    (
        StatusCode::OK,
        no_store(),
        Json(VisibilityResponse { public }),
    )
        .into_response()
}

// -- /v1/me/share* ---------------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/me/share",
    tag = "sharing",
    request_body = ShareRequest,
    responses(
        (status = 200, description = "Share granted", body = ShareResponse),
        (status = 400, description = "Cannot share with self", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller hasn't proven RSI handle ownership", body = ApiErrorBody),
        (status = 404, description = "Recipient handle not found", body = ApiErrorBody),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn add_share<U: UserStore>(
    State(users): State<Arc<U>>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    Extension(audit_query): Extension<Arc<dyn AuditQuery>>,
    auth: AuthenticatedUser,
    Json(req): Json<ShareRequest>,
) -> Response {
    if let Some(resp) = require_rsi_verified(users.as_ref(), &auth).await {
        return resp;
    }

    // Audit v2.1 §C abuse-signal: auto-pause gate. When the
    // cross-report-cluster threshold fires (see
    // `check_cross_report_cluster` below), the report handler stamps
    // `shares_paused_until` on this owner with a short ban. While that
    // timestamp is in the future, every new outbound grant is
    // rejected up-front — before the SpiceDB write, before the
    // recipient lookup, before the audit row. NULL/past timestamps
    // fall through silently. Soft-fail posture: a query hiccup logs
    // and skips the gate rather than blocking the legit caller.
    match users
        .get_shares_paused_until_by_handle(&auth.preferred_username)
        .await
    {
        Ok(Some(until)) if until > Utc::now() => {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiErrorBody {
                    error: "shares_paused".into(),
                    detail: Some(format!("paused until {}", until.to_rfc3339())),
                }),
            )
                .into_response();
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "shares_paused_until lookup failed; skipping gate");
        }
    }

    let recipient = req.recipient_handle.trim();
    if !validate_handle(recipient) {
        return err(StatusCode::BAD_REQUEST, "invalid_recipient_handle");
    }

    if recipient.eq_ignore_ascii_case(&auth.preferred_username) {
        return err(StatusCode::BAD_REQUEST, "cannot_share_with_self");
    }

    // Audit v2.1 §C abuse-signal: rapid-grant rate-limit. Soft-fail
    // posture — a hiccup in the audit query degrades to "no check"
    // rather than blocking a legitimate share. The signal row gives
    // moderators visibility even if the rate-limit doesn't fire on
    // this specific request.
    if let Some(resp) = check_rapid_grant(audit_query.as_ref(), audit.as_ref(), &auth).await {
        return resp;
    }

    // Validate optional metadata up-front so we don't write a
    // SpiceDB row and then bail on a 400. Past expiry rejected to
    // catch obvious client mistakes; empty notes collapse to None.
    let now = Utc::now();
    if let Some(t) = req.expires_at {
        if t <= now {
            return err(StatusCode::BAD_REQUEST, "expires_at_in_past");
        }
    }
    let note_owned: Option<String> = req
        .note
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(n) = note_owned.as_ref() {
        if n.chars().count() > NOTE_MAX_LEN {
            return err(StatusCode::BAD_REQUEST, "note_too_long");
        }
    }

    // Validate the optional scope. `kind = "full"` is normalised to
    // `None` (no JSONB row) so re-grants from a UI that always sends
    // a scope can still clear it back to legacy behaviour.
    let scope_owned: Option<ShareScope> = match req.scope.as_ref() {
        Some(s) => {
            if let Err(code) = validate_scope(s) {
                return err(StatusCode::BAD_REQUEST, code);
            }
            if s.kind == "full" {
                None
            } else {
                Some(s.clone())
            }
        }
        None => None,
    };
    let scope_json: Option<serde_json::Value> = scope_owned
        .as_ref()
        .and_then(|s| serde_json::to_value(s).ok());

    // Validate the recipient exists in our user table — sharing with a
    // ghost handle is a UX trap (the wildcard subject would be the only
    // way to grant such a thing, and that's the public toggle's job).
    let recipient_user = match users.find_by_handle(recipient).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return err(StatusCode::NOT_FOUND, "recipient_not_found");
        }
        Err(e) => {
            tracing::error!(error = %e, "find_by_handle failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    };

    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    if let Err(e) = client
        .write_share_with_user(&auth.preferred_username, &recipient_user.claimed_handle)
        .await
    {
        tracing::error!(error = %e, "write share failed");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
    }

    // Always upsert metadata, even when both fields are None — this
    // makes re-POST behave as a true upsert that can both set AND
    // clear an expiry or note. Without this, the /sharing edit flow
    // can set metadata via re-POST but can't remove it. Metadata is
    // best-effort: a Postgres failure here is logged but doesn't
    // roll back the already-committed SpiceDB grant.
    if let Err(e) = meta
        .upsert(
            &auth.preferred_username,
            &recipient_user.claimed_handle,
            req.expires_at,
            note_owned.as_deref(),
            scope_json.as_ref(),
        )
        .await
    {
        tracing::warn!(error = %e, "share_metadata upsert failed");
    }

    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(auth.sub.clone()),
            actor_handle: Some(auth.preferred_username.clone()),
            action: "share.granted".to_string(),
            payload: serde_json::json!({
                "recipient_handle": recipient_user.claimed_handle,
                "expires_at": req.expires_at,
                "has_note": note_owned.is_some(),
                "scope_kind": scope_owned.as_ref().map(|s| s.kind.clone()),
            }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (share grant)");
    }

    (
        StatusCode::OK,
        Json(ShareResponse {
            shared_with: recipient_user.claimed_handle,
        }),
    )
        .into_response()
}

#[utoipa::path(
    delete,
    path = "/v1/me/share/{recipient_handle}",
    tag = "sharing",
    params(
        ("recipient_handle" = String, Path, description = "RSI handle to revoke share from")
    ),
    responses(
        (status = 200, description = "Share revoked (idempotent)", body = RevokeShareResponse),
        (status = 400, description = "Invalid handle in path", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn delete_share(
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    auth: AuthenticatedUser,
    Path(recipient_handle): Path<String>,
) -> Response {
    let recipient = recipient_handle.trim();
    if !validate_handle(recipient) {
        return err(StatusCode::BAD_REQUEST, "invalid_recipient_handle");
    }

    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    if let Err(e) = client
        .delete_share_with_user(&auth.preferred_username, recipient)
        .await
    {
        tracing::error!(error = %e, "delete share failed");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
    }

    // Wipe metadata after the SpiceDB row goes away. Best-effort —
    // a leftover row with no SpiceDB relation is invisible to all
    // reads (find/list never join an orphan), so a transient
    // failure here is benign.
    if let Err(e) = meta.delete(&auth.preferred_username, recipient).await {
        tracing::warn!(error = %e, "share_metadata delete failed");
    }

    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(auth.sub.clone()),
            actor_handle: Some(auth.preferred_username.clone()),
            action: "share.revoked".to_string(),
            payload: serde_json::json!({ "recipient_handle": recipient }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (share revoke)");
    }

    (StatusCode::OK, Json(RevokeShareResponse { revoked: true })).into_response()
}

#[utoipa::path(
    get,
    path = "/v1/me/shares",
    tag = "sharing",
    responses(
        (status = 200, description = "List of recipient handles you've shared with", body = ListSharesResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn list_shares(
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
    Extension(audit_query): Extension<Arc<dyn AuditQuery>>,
    auth: AuthenticatedUser,
) -> Response {
    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    let handles = match client.list_share_with_user(&auth.preferred_username).await {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "list shares failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
        }
    };

    // Bulk-fetch metadata in one round-trip then index by lowercased
    // recipient so the join is constant-time. Failure degrades to
    // "no metadata recorded" rather than failing the whole call —
    // the SpiceDB rows are the source of truth for which shares
    // exist, metadata is decorative.
    let meta_index: std::collections::HashMap<String, _> =
        match meta.list_by_owner(&auth.preferred_username).await {
            Ok(rows) => rows
                .into_iter()
                .map(|m| (m.recipient_handle.to_ascii_lowercase(), m))
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "list_by_owner metadata fetch failed");
                std::collections::HashMap::new()
            }
        };
    // Bulk-load view stats from audit_log. Failure degrades to
    // "no stats" (view_count = 0) rather than failing the whole call
    // — the same posture as metadata fetch.
    let view_index: std::collections::HashMap<String, _> = match audit_query
        .share_views_for_owner(&auth.preferred_username)
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .map(|s| (s.recipient_handle.to_ascii_lowercase(), s))
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "share_views_for_owner fetch failed");
            std::collections::HashMap::new()
        }
    };
    let user_shares: Vec<ShareEntry> = handles
        .into_iter()
        .map(|h| {
            let key = h.to_ascii_lowercase();
            let m = meta_index.get(&key);
            let v = view_index.get(&key);
            ShareEntry {
                recipient_handle: h,
                expires_at: m.and_then(|x| x.expires_at),
                note: m.and_then(|x| x.note.clone()),
                scope: m.and_then(|x| x.scope.as_ref()).and_then(scope_from_value),
                view_count: v.map(|s| s.view_count).unwrap_or(0),
                last_viewed_at: v.and_then(|s| s.last_viewed_at),
            }
        })
        .collect();
    let org_shares = match client.list_share_with_org(&auth.preferred_username).await {
        Ok(slugs) => slugs
            .into_iter()
            .map(|org_slug| OrgShareEntry { org_slug })
            .collect::<Vec<_>>(),
        Err(e) => {
            // The user-share half already succeeded; degrade by
            // returning an empty org list rather than failing the
            // whole call. Logged so ops sees the partial outage.
            tracing::warn!(
                error = %e,
                "list org shares failed; returning empty org_shares"
            );
            Vec::new()
        }
    };
    (
        StatusCode::OK,
        no_store(),
        Json(ListSharesResponse {
            shares: user_shares,
            org_shares,
        }),
    )
        .into_response()
}

// -- /v1/me/shared-with-me -----------------------------------------

#[utoipa::path(
    get,
    path = "/v1/me/shared-with-me",
    tag = "sharing",
    responses(
        (
            status = 200,
            description = "List of owner handles who have shared their stats_record with the caller",
            body = ListSharedWithMeResponse,
        ),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn list_shared_with_me(
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
    auth: AuthenticatedUser,
) -> Response {
    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    let handles = match client.list_shared_with_me(&auth.preferred_username).await {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "list_shared_with_me failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
        }
    };

    let meta_index: std::collections::HashMap<String, _> =
        match meta.list_by_recipient(&auth.preferred_username).await {
            Ok(rows) => rows
                .into_iter()
                .map(|m| (m.owner_handle.to_ascii_lowercase(), m))
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "list_by_recipient metadata fetch failed");
                std::collections::HashMap::new()
            }
        };
    let owners: Vec<SharedWithMeEntry> = handles
        .into_iter()
        .map(|h| {
            let m = meta_index.get(&h.to_ascii_lowercase());
            SharedWithMeEntry {
                owner_handle: h,
                expires_at: m.and_then(|x| x.expires_at),
                note: m.and_then(|x| x.note.clone()),
                scope: m.and_then(|x| x.scope.as_ref()).and_then(scope_from_value),
            }
        })
        .collect();

    (
        StatusCode::OK,
        no_store(),
        Json(ListSharedWithMeResponse {
            shared_with_me: owners,
        }),
    )
        .into_response()
}

// -- /v1/me/share/org ------------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/me/share/org",
    tag = "sharing",
    request_body = ShareOrgRequest,
    responses(
        (status = 200, description = "Share with org granted", body = ShareOrgResponse),
        (status = 400, description = "Invalid org slug", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller hasn't proven RSI handle ownership", body = ApiErrorBody),
        (status = 404, description = "Org not found", body = ApiErrorBody),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn share_with_org<O: OrgStore, U: UserStore>(
    State((orgs, users)): State<(Arc<O>, Arc<U>)>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    auth: AuthenticatedUser,
    Json(req): Json<ShareOrgRequest>,
) -> Response {
    if let Some(resp) = require_rsi_verified(users.as_ref(), &auth).await {
        return resp;
    }

    let slug = req.org_slug.trim();
    if slug.is_empty()
        || slug.len() > crate::orgs::SLUG_MAX_LEN
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return err(StatusCode::BAD_REQUEST, "invalid_org_slug");
    }

    // Validate the org exists in our metadata table — sharing with a
    // ghost slug is the same trap as sharing with a ghost handle.
    let org = match orgs.find_by_slug(slug).await {
        Ok(Some(o)) => o,
        Ok(None) => return err(StatusCode::NOT_FOUND, "org_not_found"),
        Err(e) => {
            tracing::error!(error = %e, "find_by_slug failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    };

    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    if let Err(e) = client
        .write_share_with_org(&auth.preferred_username, &org.slug)
        .await
    {
        tracing::error!(error = %e, "write_share_with_org failed");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
    }

    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(auth.sub.clone()),
            actor_handle: Some(auth.preferred_username.clone()),
            action: "share.org_granted".to_string(),
            payload: serde_json::json!({ "org_slug": org.slug }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (share.org_granted)");
    }

    (
        StatusCode::OK,
        Json(ShareOrgResponse {
            shared_with_org: org.slug,
        }),
    )
        .into_response()
}

#[utoipa::path(
    delete,
    path = "/v1/me/share/org/{slug}",
    tag = "sharing",
    params(("slug" = String, Path, description = "Org slug to revoke share from")),
    responses(
        (status = 200, description = "Org share revoked (idempotent)", body = RevokeOrgShareResponse),
        (status = 400, description = "Invalid org slug", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn unshare_with_org(
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    auth: AuthenticatedUser,
    Path(slug): Path<String>,
) -> Response {
    let s = slug.trim();
    if s.is_empty()
        || s.len() > crate::orgs::SLUG_MAX_LEN
        || !s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return err(StatusCode::BAD_REQUEST, "invalid_org_slug");
    }

    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    if let Err(e) = client
        .delete_share_with_org(&auth.preferred_username, s)
        .await
    {
        tracing::error!(error = %e, "delete_share_with_org failed");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "spicedb_error");
    }

    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(auth.sub.clone()),
            actor_handle: Some(auth.preferred_username.clone()),
            action: "share.org_revoked".to_string(),
            payload: serde_json::json!({ "org_slug": s }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (share.org_revoked)");
    }

    (
        StatusCode::OK,
        Json(RevokeOrgShareResponse { revoked: true }),
    )
        .into_response()
}

// -- Public read endpoints ------------------------------------------

/// Check `public_view` on `stats_record:<handle>`. Returns `Ok(bool)`
/// on a successful permission lookup, `Err(_)` when SpiceDB is
/// unreachable / errored — callers map that to 503 rather than 404,
/// so an outage doesn't masquerade as "handle not public".
async fn check_public(client: &SpicedbClient, handle: &str) -> anyhow::Result<bool> {
    let resource = ObjectRef::new("stats_record", handle);
    let subject = ObjectRef::new("user", "*");
    client
        .check_permission(resource, "view", subject)
        .await
        .inspect_err(|e| tracing::warn!(error = %e, "spicedb public check failed"))
}

/// Check `view` permission for `viewer` on `stats_record:<owner>`.
/// Same outage posture as [`check_public`]: errors bubble so the
/// handler can return 503 instead of a misleading 404.
async fn check_view(client: &SpicedbClient, owner: &str, viewer: &str) -> anyhow::Result<bool> {
    let resource = ObjectRef::new("stats_record", owner);
    let subject = ObjectRef::new("user", viewer);
    client
        .check_permission(resource, "view", subject)
        .await
        .inspect_err(|e| tracing::warn!(error = %e, "spicedb friend check failed"))
}

/// Wrap [`check_view`] with read-time expiry enforcement. After the
/// SpiceDB permission check succeeds, look up `share_metadata` for
/// the (owner, viewer) pair. If `expires_at` is set and in the
/// past, lazy-revoke: delete the SpiceDB row + the metadata row +
/// audit, and return `Ok(false)` so the caller renders a 404
/// (matches the "share never existed" UX and avoids leaking the
/// expiration state).
///
/// `audit` is best-effort — the share is already revoked logically
/// once metadata says so, so an audit hiccup doesn't reverse the
/// effective decision.
async fn check_view_with_expiry(
    client: &SpicedbClient,
    meta: &dyn ShareMetadataStore,
    audit: &dyn AuditLog,
    owner: &str,
    viewer: &str,
) -> anyhow::Result<bool> {
    let allowed = check_view(client, owner, viewer).await?;
    if !allowed {
        return Ok(false);
    }
    let row = match meta.find(owner, viewer).await {
        Ok(r) => r,
        Err(e) => {
            // Metadata fetch failed but SpiceDB said "allowed" — fail
            // open. The share works without metadata; we'd rather not
            // 404 a legitimate viewer because Postgres flapped.
            tracing::warn!(error = %e, "share_metadata find failed; defaulting to allow");
            return Ok(true);
        }
    };
    let Some(meta_row) = row else {
        return Ok(true);
    };
    let Some(expires_at) = meta_row.expires_at else {
        return Ok(true);
    };
    if expires_at > Utc::now() {
        return Ok(true);
    }
    // Expired — lazy-revoke. Errors here are warnings, not fatal:
    // even if cleanup partially fails, the next read will retry.
    if let Err(e) = client.delete_share_with_user(owner, viewer).await {
        tracing::warn!(error = %e, "lazy spicedb revoke (expired share) failed");
    }
    if let Err(e) = meta.delete(owner, viewer).await {
        tracing::warn!(error = %e, "lazy metadata delete (expired share) failed");
    }
    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: None,
            actor_handle: Some(owner.to_string()),
            action: "share.expired_revoked".to_string(),
            payload: serde_json::json!({
                "recipient_handle": viewer,
                "expired_at": expires_at,
            }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (share.expired_revoked)");
    }
    Ok(false)
}

/// Convert `check_public`/`check_view` results into a handler response.
/// `Ok(true)` runs `then` (the 200 path); `Ok(false)` is 404 (don't
/// leak existence); `Err(_)` is 503 with the standard
/// `spicedb_unavailable` error body.
async fn render_or_404<F, Fut>(check: anyhow::Result<bool>, then: F) -> Response
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Response>,
{
    match check {
        Ok(true) => then().await,
        Ok(false) => (StatusCode::NOT_FOUND, ()).into_response(),
        Err(_) => err(StatusCode::SERVICE_UNAVAILABLE, "spicedb_unavailable"),
    }
}

async fn render_summary<Q: EventQuery>(query: &Q, handle: &str) -> Response {
    render_summary_scoped(query, handle, None).await
}

/// Same as [`render_summary`] but applies a per-share scope clamp to
/// the result. The clamp drops disallowed event types from `by_type`
/// and recomputes `total` so the returned shape is internally
/// consistent (no `sum(by_type) != total` mismatch on the client).
async fn render_summary_scoped<Q: EventQuery>(
    query: &Q,
    handle: &str,
    scope: Option<&ShareScope>,
) -> Response {
    // `_shared` variant — excludes rows the owner has hidden from
    // shared/public views. Hidden events still count for the owner
    // via `/v1/me/summary` (which calls the un-suffixed method) so
    // they don't disappear from your own UI.
    match query.summary_for_handle_shared(handle).await {
        Ok((total, by_type)) => {
            let (total, by_type) = if scope.is_some() {
                apply_event_type_filter(by_type, scope)
            } else {
                (total, by_type)
            };
            (
                StatusCode::OK,
                Json(PublicSummaryResponse {
                    claimed_handle: handle.to_string(),
                    total,
                    by_type: by_type
                        .into_iter()
                        .map(|(event_type, count)| PublicTypeCount { event_type, count })
                        .collect(),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "public summary query failed");
            err(StatusCode::INTERNAL_SERVER_ERROR, "query_failed")
        }
    }
}

async fn render_timeline<Q: EventQuery>(query: &Q, handle: &str, days: u32) -> Response {
    render_timeline_scoped(query, handle, days, None).await
}

/// Scope-aware timeline. Clamps both the window (`scope.window_days`,
/// applied upstream via [`clamp_days`]) AND the per-event type stream
/// (`scope.allow_event_types` / `scope.deny_event_types`, applied
/// here by routing to the repo's `timeline_shared_filtered`). The
/// allowlist wins by precedence — types absent from a non-empty
/// allowlist are dropped before the denylist is consulted, matching
/// the summary clamp's [`apply_event_type_filter`] semantics. With
/// no scope or no per-type lists this is identical to the un-scoped
/// `render_timeline` path.
async fn render_timeline_scoped<Q: EventQuery>(
    query: &Q,
    handle: &str,
    days: u32,
    scope: Option<&ShareScope>,
) -> Response {
    let allow = scope.and_then(|s| s.allow_event_types.as_deref());
    let deny = scope.and_then(|s| s.deny_event_types.as_deref());
    // `_shared` variant — see `render_summary` for the rationale.
    let result = if allow.is_none() && deny.is_none() {
        query.timeline_shared(handle, days).await
    } else {
        query
            .timeline_shared_filtered(handle, days, allow, deny)
            .await
    };
    match result {
        Ok(rows) => {
            let buckets = build_timeline_buckets(rows, days)
                .into_iter()
                .map(|(date, count)| PublicTimelineBucket { date, count })
                .collect();
            (
                StatusCode::OK,
                Json(PublicTimelineResponse { days, buckets }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "public timeline query failed");
            err(StatusCode::INTERNAL_SERVER_ERROR, "query_failed")
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/public/{handle}/summary",
    tag = "sharing",
    params(("handle" = String, Path, description = "RSI handle to fetch public summary for")),
    responses(
        (status = 200, description = "Public summary", body = PublicSummaryResponse),
        (status = 404, description = "Not public or unknown handle"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
)]
pub async fn public_summary<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Path(handle): Path<String>,
) -> Response {
    if !validate_handle(&handle) {
        return (StatusCode::NOT_FOUND, ()).into_response();
    }
    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };

    let check = check_public(client, &handle).await;
    render_or_404(check, || async {
        render_summary(query.as_ref(), &handle).await
    })
    .await
}

#[utoipa::path(
    get,
    path = "/v1/public/{handle}/timeline",
    tag = "sharing",
    params(
        ("handle" = String, Path, description = "RSI handle"),
        PublicTimelineParams,
    ),
    responses(
        (status = 200, description = "Public timeline", body = PublicTimelineResponse),
        (status = 404, description = "Not public or unknown handle"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
)]
pub async fn public_timeline<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Path(handle): Path<String>,
    Query(params): Query<PublicTimelineParams>,
) -> Response {
    if !validate_handle(&handle) {
        return (StatusCode::NOT_FOUND, ()).into_response();
    }
    let Ok(days) = resolve_timeline_days(params.days) else {
        return err(StatusCode::BAD_REQUEST, "invalid_days");
    };
    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };
    let check = check_public(client, &handle).await;
    render_or_404(check, || async {
        render_timeline(query.as_ref(), &handle, days).await
    })
    .await
}

#[utoipa::path(
    get,
    path = "/v1/u/{handle}/summary",
    tag = "sharing",
    params(("handle" = String, Path, description = "Owner RSI handle")),
    responses(
        (status = 200, description = "Friend summary", body = PublicSummaryResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 404, description = "Not shared with you or unknown handle"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn friend_summary<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    auth: AuthenticatedUser,
    Path(handle): Path<String>,
) -> Response {
    if !validate_handle(&handle) {
        return (StatusCode::NOT_FOUND, ()).into_response();
    }
    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };
    let check = check_view_with_expiry(
        client,
        meta.as_ref(),
        audit.as_ref(),
        &handle,
        &auth.preferred_username,
    )
    .await;
    // Pull the scope clamp now, after the auth gate. A metadata
    // fetch failure degrades to "no clamp" rather than 404ing the
    // viewer — same posture as the expiry check, which also fails
    // open when Postgres flaps.
    let scope = meta
        .find(&handle, &auth.preferred_username)
        .await
        .ok()
        .flatten()
        .and_then(|m| m.scope)
        .and_then(|v| scope_from_value(&v));
    if let Some(s) = scope.as_ref() {
        if !scope_allows_aggregates(s) {
            return (StatusCode::NOT_FOUND, ()).into_response();
        }
    }
    render_or_404(check, || async {
        emit_share_viewed(audit.as_ref(), &auth, &handle).await;
        render_summary_scoped(query.as_ref(), &handle, scope.as_ref()).await
    })
    .await
}

#[utoipa::path(
    get,
    path = "/v1/u/{handle}/timeline",
    tag = "sharing",
    params(
        ("handle" = String, Path, description = "Owner RSI handle"),
        PublicTimelineParams,
    ),
    responses(
        (status = 200, description = "Friend timeline", body = PublicTimelineResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 404, description = "Not shared with you or unknown handle"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn friend_timeline<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Extension(meta): Extension<Arc<dyn ShareMetadataStore>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    auth: AuthenticatedUser,
    Path(handle): Path<String>,
    Query(params): Query<PublicTimelineParams>,
) -> Response {
    if !validate_handle(&handle) {
        return (StatusCode::NOT_FOUND, ()).into_response();
    }
    let Ok(days) = resolve_timeline_days(params.days) else {
        return err(StatusCode::BAD_REQUEST, "invalid_days");
    };
    let Some(client) = spicedb.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                error: "spicedb_unavailable".into(),
                detail: None,
            }),
        )
            .into_response();
    };
    let check = check_view_with_expiry(
        client,
        meta.as_ref(),
        audit.as_ref(),
        &handle,
        &auth.preferred_username,
    )
    .await;
    let scope = meta
        .find(&handle, &auth.preferred_username)
        .await
        .ok()
        .flatten()
        .and_then(|m| m.scope)
        .and_then(|v| scope_from_value(&v));
    if let Some(s) = scope.as_ref() {
        if !scope_allows_timeline(s) {
            return (StatusCode::NOT_FOUND, ()).into_response();
        }
    }
    let clamped_days = clamp_days(days, scope.as_ref());
    render_or_404(check, || async {
        emit_share_viewed(audit.as_ref(), &auth, &handle).await;
        render_timeline_scoped(query.as_ref(), &handle, clamped_days, scope.as_ref()).await
    })
    .await
}

// -- /v1/share/report ------------------------------------------------

/// Request body for `POST /v1/share/report`. Reporter is the auth'd
/// user (NOT a body field) — taking it off the token prevents
/// spoofing one user as another.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ReportShareRequest {
    /// Owner side of the (owner, recipient) pair being reported.
    pub owner_handle: String,
    /// Recipient side of the (owner, recipient) pair being reported.
    pub recipient_handle: String,
    /// One of `abuse | spam | data_misuse | other`. Other values 400.
    pub reason: String,
    /// Optional free-text context. Capped at `DETAILS_MAX_LEN` chars.
    pub details: Option<String>,
}

/// Echo of the created report so the UI can confirm the row landed.
/// Tight subset of `ShareReport` — full row is admin-only.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportShareResponse {
    pub id: uuid::Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// POST `/v1/share/report` — file a moderation report against a
/// specific (owner_handle, recipient_handle) share.
///
/// Authorization model: the reporter (auth'd user) must be one side
/// of the share. Either the recipient flagging the owner ("creep
/// shared with me") or the owner flagging the recipient ("recipient
/// is doing creepy stuff with my data"). A third party can't report.
///
/// Rate-limited at the app layer: max `RATE_LIMIT_PER_WINDOW` rows
/// per reporter per `rate_limit_window()`. The handler queries the
/// store; no Redis dependency. Exceeded -> 429.
#[utoipa::path(
    post,
    path = "/v1/share/report",
    tag = "sharing",
    request_body = ReportShareRequest,
    responses(
        (status = 200, description = "Report filed", body = ReportShareResponse),
        (status = 400, description = "Invalid handle, reason, or details length", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is neither the owner nor recipient of the share", body = ApiErrorBody),
        (status = 429, description = "Reporter rate-limit exceeded", body = ApiErrorBody),
        (status = 500, description = "Database error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn report_share(
    auth: AuthenticatedUser,
    Extension(reports): Extension<Arc<dyn ShareReportStore>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    Extension(users): Extension<Arc<dyn UserStore>>,
    Json(body): Json<ReportShareRequest>,
) -> Response {
    // Validate handles (cheap, same rules as the rest of the file).
    if !validate_handle(&body.owner_handle) || !validate_handle(&body.recipient_handle) {
        return err(StatusCode::BAD_REQUEST, "invalid_handle");
    }
    let reason = match ShareReportReason::parse(&body.reason) {
        Some(r) => r,
        None => return err(StatusCode::BAD_REQUEST, "invalid_reason"),
    };
    let details = match body.details.as_deref().map(str::trim) {
        Some("") => None,
        Some(s) if s.chars().count() > DETAILS_MAX_LEN => {
            return err(StatusCode::BAD_REQUEST, "details_too_long")
        }
        Some(s) => Some(s.to_string()),
        None => None,
    };

    // Authorization gate: reporter must be one side of the share.
    let reporter = &auth.preferred_username;
    let is_owner_side = reporter.eq_ignore_ascii_case(&body.owner_handle);
    let is_recipient_side = reporter.eq_ignore_ascii_case(&body.recipient_handle);
    if !is_owner_side && !is_recipient_side {
        return err(StatusCode::FORBIDDEN, "not_a_party_to_the_share");
    }

    // Rate-limit by reporter. Best-effort: a transient DB hiccup here
    // shouldn't block a legitimate report, so we log + skip the gate
    // if the count itself fails.
    let since = chrono::Utc::now() - rate_limit_window();
    match reports.count_recent_by_reporter(reporter, since).await {
        Ok(n) if n >= RATE_LIMIT_PER_WINDOW => {
            return err(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "share_reports rate-limit count failed; skipping gate");
        }
    }

    let row = match reports
        .create(
            reporter,
            &body.owner_handle,
            &body.recipient_handle,
            reason,
            details.as_deref(),
        )
        .await
    {
        Ok(r) => r,
        Err(ShareReportError::Database(e)) => {
            tracing::error!(error = %e, "share_reports.create failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "database_error");
        }
        Err(e) => {
            tracing::error!(error = %e, "share_reports.create unexpected");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    };

    // Best-effort audit emission. Same posture as `share.viewed` —
    // a hiccup here must not poison the response.
    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(auth.sub.clone()),
            actor_handle: Some(reporter.clone()),
            action: "share.reported".to_string(),
            payload: serde_json::json!({
                "report_id": row.id,
                "owner_handle": row.owner_handle,
                "recipient_handle": row.recipient_handle,
                "reason": row.reason.as_str(),
            }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (share.reported)");
    }

    // Audit v2.1 §C abuse-signal: cross-report cluster. Once the
    // owner accumulates >= CLUSTER_REPORT_THRESHOLD reports inside
    // CLUSTER_WINDOW, emit a signal row AND stamp
    // `users.shares_paused_until` with a short ban (PAUSE_DURATION).
    // The signal row gives moderators visibility; the column flip
    // gates `add_share` for the duration. Both writes are best-effort
    // — a DB hiccup on either degrades to a logged warning, not a
    // poisoned response.
    check_cross_report_cluster(
        reports.as_ref(),
        audit.as_ref(),
        users.as_ref(),
        &row.owner_handle,
    )
    .await;

    (
        StatusCode::OK,
        Json(ReportShareResponse {
            id: row.id,
            status: row.status.as_str().to_string(),
            created_at: row.created_at,
        }),
    )
        .into_response()
}

// -- Audit v2.1 §C — abuse-signal helpers ----------------------------

/// Soft cap on `share.created` rows the same actor may write inside
/// the RAPID_GRANT_WINDOW. Crossing the threshold returns 429 to the
/// caller AND emits a `share.signal_rapid_grant` audit row so
/// moderators see the pattern. Audit v2.1 §C documents these as
/// "starting values — tune after a week of production data".
const RAPID_GRANT_THRESHOLD: usize = 15;
const RAPID_GRANT_WINDOW_HOURS: i64 = 24;

/// Cross-report cluster: same number of reports against one owner
/// inside CLUSTER_WINDOW emits a signal row AND stamps
/// `users.shares_paused_until = now() + PAUSE_DURATION_HOURS` so the
/// next `add_share` from that owner returns 403 `shares_paused`. The
/// pause is short by design — long enough to deter a flood, short
/// enough that NULL'ing the column manually is rarely needed. A
/// moderator can still clear it early via the admin surface.
const CLUSTER_REPORT_THRESHOLD: usize = 3;
const CLUSTER_WINDOW_HOURS: i64 = 72;
const PAUSE_DURATION_HOURS: i64 = 24;

async fn check_rapid_grant(
    audit_query: &dyn AuditQuery,
    audit: &dyn AuditLog,
    auth: &AuthenticatedUser,
) -> Option<Response> {
    let since = Utc::now() - chrono::Duration::hours(RAPID_GRANT_WINDOW_HOURS);
    // Limit just past the threshold — `.len()` is the count we care
    // about and we don't need to page the whole window.
    let rows = match audit_query
        .list(crate::audit::AuditFilters {
            actor_handle: Some(auth.preferred_username.clone()),
            action: Some("share.created".to_string()),
            since: Some(since),
            until: None,
            limit: (RAPID_GRANT_THRESHOLD as i64) + 1,
            offset: 0,
        })
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "rapid-grant check failed; skipping gate");
            return None;
        }
    };
    if rows.len() < RAPID_GRANT_THRESHOLD {
        return None;
    }
    // Threshold crossed — emit the signal row, then return 429.
    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: Some(auth.sub.clone()),
            actor_handle: Some(auth.preferred_username.clone()),
            action: "share.signal_rapid_grant".to_string(),
            payload: serde_json::json!({
                "window_hours": RAPID_GRANT_WINDOW_HOURS,
                "threshold": RAPID_GRANT_THRESHOLD,
                "observed_count": rows.len(),
            }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (signal_rapid_grant)");
    }
    Some(err(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited_rapid_grant",
    ))
}

async fn check_cross_report_cluster(
    reports: &dyn ShareReportStore,
    audit: &dyn AuditLog,
    users: &dyn UserStore,
    owner_handle: &str,
) {
    let since = Utc::now() - chrono::Duration::hours(CLUSTER_WINDOW_HOURS);
    // No by-subject filter on the store — list the recent population
    // and filter in memory. At homelab volume the 500-row cap is a
    // comfortable upper bound; same posture as the by-user admin
    // endpoint.
    let all = match reports.list(None, 500, 0).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "cluster check list failed; skipping");
            return;
        }
    };
    let owner_lc = owner_handle.to_ascii_lowercase();
    let n = all
        .iter()
        .filter(|r| r.owner_handle.to_ascii_lowercase() == owner_lc)
        .filter(|r| r.created_at >= since)
        .count();
    if n < CLUSTER_REPORT_THRESHOLD {
        return;
    }

    // Stamp the pause first so the audit row's `paused_until` field
    // reflects the timestamp we actually wrote. If the column write
    // fails, the audit row still goes out with `auto_pause_applied:
    // false` so moderators see the detection even when enforcement
    // misfires.
    let paused_until = Utc::now() + chrono::Duration::hours(PAUSE_DURATION_HOURS);
    let pause_applied = match users
        .set_shares_paused_until_by_handle(owner_handle, Some(paused_until))
        .await
    {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(error = %e, "shares_paused_until stamp failed; audit-only");
            false
        }
    };

    if let Err(e) = audit
        .append(AuditEntry {
            actor_sub: None,
            actor_handle: Some(owner_handle.to_string()),
            action: "share.signal_cluster_pause".to_string(),
            payload: serde_json::json!({
                "window_hours": CLUSTER_WINDOW_HOURS,
                "threshold": CLUSTER_REPORT_THRESHOLD,
                "observed_count": n,
                "auto_pause_applied": pause_applied,
                "paused_until": pause_applied.then_some(paused_until),
                "pause_duration_hours": PAUSE_DURATION_HOURS,
            }),
        })
        .await
    {
        tracing::warn!(error = %e, "audit log append failed (signal_cluster_pause)");
    }
}

// -- /v1/me/preview-share/* ------------------------------------------
//
// Audit v2.1 §B1 — "Preview as @handle" — simulated render path.
//
// The owner configures a scope, hits Preview, and gets a new tab
// showing their OWN data run through that scope's filter. No
// SpiceDB check (the owner is reading their own data — no friend
// gate to satisfy). No audit row (this is a simulation, not a real
// view). The recipient handle is purely cosmetic for the banner
// the frontend renders — server-side it doesn't shape the response.

#[derive(Debug, Deserialize, IntoParams)]
pub struct PreviewSummaryParams {
    /// URL-encoded JSON of a [`ShareScope`]. Missing/blank = render
    /// without any scope clamp (equivalent to a full-manifest preview).
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct PreviewTimelineParams {
    /// Timeline window in days; same semantics as
    /// [`PublicTimelineParams::days`]. Clamped against `scope.window_days`
    /// if both are set.
    pub days: Option<u32>,
    /// URL-encoded JSON of a [`ShareScope`].
    pub scope: Option<String>,
}

fn parse_scope_param(raw: Option<&str>) -> Result<Option<ShareScope>, Response> {
    let Some(s) = raw else { return Ok(None) };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<ShareScope>(trimmed) {
        Ok(scope) => Ok(Some(scope)),
        Err(_) => Err(err(StatusCode::BAD_REQUEST, "invalid_scope")),
    }
}

#[utoipa::path(
    get,
    path = "/v1/me/preview-share/summary",
    tag = "sharing",
    params(PreviewSummaryParams),
    responses(
        (status = 200, description = "Owner summary clamped by the supplied scope", body = PublicSummaryResponse),
        (status = 400, description = "Scope JSON failed to parse"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn preview_summary<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    auth: AuthenticatedUser,
    Query(params): Query<PreviewSummaryParams>,
) -> Response {
    let scope = match parse_scope_param(params.scope.as_deref()) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    if let Some(s) = scope.as_ref() {
        if !scope_allows_aggregates(s) {
            // For preview, return an empty 200 rather than 404 so the
            // page can render the banner + an explanatory empty state.
            return (
                StatusCode::OK,
                Json(PublicSummaryResponse {
                    claimed_handle: auth.preferred_username.clone(),
                    total: 0,
                    by_type: vec![],
                }),
            )
                .into_response();
        }
    }
    render_summary_scoped(query.as_ref(), &auth.preferred_username, scope.as_ref()).await
}

#[utoipa::path(
    get,
    path = "/v1/me/preview-share/timeline",
    tag = "sharing",
    params(PreviewTimelineParams),
    responses(
        (status = 200, description = "Owner timeline clamped by the supplied scope", body = PublicTimelineResponse),
        (status = 400, description = "Scope JSON failed to parse or days out of range"),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn preview_timeline<Q: EventQuery>(
    State(query): State<Arc<Q>>,
    auth: AuthenticatedUser,
    Query(params): Query<PreviewTimelineParams>,
) -> Response {
    let scope = match parse_scope_param(params.scope.as_deref()) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let Ok(days) = resolve_timeline_days(params.days) else {
        return err(StatusCode::BAD_REQUEST, "invalid_days");
    };
    let days = clamp_days(days, scope.as_ref());
    if let Some(s) = scope.as_ref() {
        if !scope_allows_timeline(s) {
            return (
                StatusCode::OK,
                Json(PublicTimelineResponse {
                    days,
                    buckets: vec![],
                }),
            )
                .into_response();
        }
    }
    render_timeline_scoped(
        query.as_ref(),
        &auth.preferred_username,
        days,
        scope.as_ref(),
    )
    .await
}

// -- Tests -----------------------------------------------------------
//
// SpiceDB writes/reads need a live sidecar to round-trip cleanly, and
// extracting `SpicedbClient` behind a trait would force an allocation
// + dyn-dispatch on the hot path of `query::summary` (which the wave
// 1 code already exercises directly). The tests below therefore skip
// the SpiceDB-touching paths and exercise the validation + audit
// behaviour, which is where the bug surface actually lives.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::test_support::MemoryAuditLog;
    use crate::auth::test_support::fresh_pair;
    use crate::auth::AuthVerifier;
    use crate::share_metadata::test_support::MemoryShareMetadataStore;
    use crate::users::test_support::MemoryUserStore;
    use crate::users::{hash_password, UserStore};
    use axum::body::to_bytes;
    use axum::http::Request;
    use axum::routing::{delete, post};
    use axum::Router;
    use tower::ServiceExt;
    use uuid::Uuid;

    fn router(
        users: Arc<MemoryUserStore>,
        verifier: Arc<AuthVerifier>,
        spicedb: Arc<Option<SpicedbClient>>,
        audit: Arc<dyn AuditLog>,
    ) -> Router {
        let meta: Arc<dyn ShareMetadataStore> = Arc::new(MemoryShareMetadataStore::default());
        // `list_shares` (added in W3) reads view stats off the same
        // memory audit log, so reuse the writer's storage by sharing
        // a fresh MemoryAuditLog through both Extensions. Tests that
        // care about *writes* still pass their own writer in via
        // `audit`; tests that don't touch /v1/me/shares are unaffected.
        let audit_query_log: Arc<MemoryAuditLog> = Arc::new(MemoryAuditLog::default());
        let audit_query: Arc<dyn AuditQuery> = audit_query_log;
        Router::new()
            .route("/v1/me/visibility", post(set_visibility::<MemoryUserStore>))
            .route("/v1/me/share", post(add_share::<MemoryUserStore>))
            .route("/v1/me/share/:recipient_handle", delete(delete_share))
            .route("/v1/me/shared-with-me", get(list_shared_with_me))
            .route("/v1/me/shares", get(list_shares))
            .with_state(users)
            .layer(Extension(verifier))
            .layer(Extension(spicedb))
            .layer(Extension(meta))
            .layer(Extension(audit))
            .layer(Extension(audit_query))
    }

    /// Seed a user, mark their RSI handle verified, and return the
    /// `(user_id, bearer)` pair. The verified mark is incidental —
    /// these tests are not about the gate, so seeding pre-verified
    /// keeps the assertions focused on whatever the test actually
    /// exercises.
    async fn seed_user(
        store: &MemoryUserStore,
        email: &str,
        handle: &str,
        issuer: &crate::auth::TokenIssuer,
    ) -> (Uuid, String) {
        let phc = hash_password("password-123-abcdef").unwrap();
        let user = store.create(email, &phc, handle).await.unwrap();
        store.mark_rsi_verified(user.id).await.unwrap();
        let token = issuer
            .sign_user(&user.id.to_string(), handle)
            .expect("sign user token");
        (user.id, token)
    }

    async fn read_body(resp: axum::response::Response) -> (StatusCode, serde_json::Value) {
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, v)
    }

    #[tokio::test]
    async fn set_visibility_returns_503_when_spicedb_skipped() {
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit: Arc<dyn AuditLog> = Arc::new(MemoryAuditLog::default());
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        let app = router(users, Arc::new(verifier), spicedb, audit);
        let req = Request::builder()
            .method("POST")
            .uri("/v1/me/visibility")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"public":true}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "spicedb_unavailable");
    }

    #[tokio::test]
    async fn share_with_unknown_handle_returns_404() {
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit_mem = Arc::new(MemoryAuditLog::default());
        let audit: Arc<dyn AuditLog> = audit_mem.clone();
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        let app = router(users, Arc::new(verifier), spicedb, audit);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/me/share")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"recipient_handle":"NobodyHere"}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "recipient_not_found");
        // No audit row should have been written because the validation
        // failed before the SpiceDB write would have run.
        assert!(audit_mem.snapshot().is_empty());
    }

    #[tokio::test]
    async fn share_with_self_returns_400() {
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit_mem = Arc::new(MemoryAuditLog::default());
        let audit: Arc<dyn AuditLog> = audit_mem.clone();
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        let app = router(users, Arc::new(verifier), spicedb, audit);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/me/share")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"recipient_handle":"alice"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "cannot_share_with_self");
        assert!(audit_mem.snapshot().is_empty());
    }

    #[tokio::test]
    async fn add_share_returns_403_when_owner_is_paused() {
        // Audit v2.1 §C: when shares_paused_until is in the future, the
        // gate fires BEFORE the recipient lookup / SpiceDB write — so a
        // 403 shares_paused short-circuits even with no spicedb wired,
        // no recipient seeded, and a malformed body. That's the whole
        // point of stamping the column at the top of add_share.
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit_mem = Arc::new(MemoryAuditLog::default());
        let audit: Arc<dyn AuditLog> = audit_mem.clone();
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        users
            .set_shares_paused_until_by_handle(
                "Alice",
                Some(Utc::now() + chrono::Duration::hours(1)),
            )
            .await
            .unwrap();
        let app = router(users, Arc::new(verifier), spicedb, audit);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/me/share")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"recipient_handle":"bob"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "shares_paused");
        // No audit row for a refused grant — only the pause-fire path
        // writes one, and that's not exercised here.
        assert!(audit_mem.snapshot().is_empty());
    }

    #[tokio::test]
    async fn add_share_passes_pause_gate_when_expired() {
        // Past timestamps fall through silently — the gate is a check
        // against "is the ban currently active?", not "has this user
        // ever been paused?". A past timestamp should NOT short-circuit;
        // the request continues until the next validation gate (here,
        // the missing spicedb sidecar → 503).
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit: Arc<dyn AuditLog> = Arc::new(MemoryAuditLog::default());
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        // Stamp a past timestamp — the gate should NOT fire.
        users
            .set_shares_paused_until_by_handle(
                "Alice",
                Some(Utc::now() - chrono::Duration::hours(1)),
            )
            .await
            .unwrap();
        // Seed the recipient too — otherwise we'd 404 at the
        // recipient lookup, which would also be a pass for this test
        // but obscures whether we hit the pause gate first.
        let phc = hash_password("password-123-abcdef").unwrap();
        users.create("bob@example.com", &phc, "bob").await.unwrap();
        let app = router(users, Arc::new(verifier), spicedb, audit);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/me/share")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"recipient_handle":"bob"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        // Got past the pause gate; the next gate (no spicedb sidecar
        // wired in the test router) returns 503.
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "spicedb_unavailable");
    }

    #[tokio::test]
    async fn delete_share_invalid_handle_returns_400() {
        // Path-segment validation runs before any SpiceDB call, so this
        // exercises the validation gate without needing a sidecar.
        // delete_share itself is *not* gated on rsi-verified (read /
        // cleanup ops aren't), so the user doesn't need to be seeded —
        // but the token still needs a real UUID sub for the auth
        // extractor's downstream handlers, hence the seed.
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit: Arc<dyn AuditLog> = Arc::new(MemoryAuditLog::default());
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        let app = router(users, Arc::new(verifier), spicedb, audit);

        // A handle containing illegal chars (`/` would split the path,
        // so use `$` which is allowed in URIs but not in our handle
        // regex).
        let req = Request::builder()
            .method("DELETE")
            .uri("/v1/me/share/bad$handle")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_recipient_handle");
    }

    #[tokio::test]
    async fn delete_share_returns_503_when_spicedb_skipped_idempotent_shape() {
        // Verifies the no-spicedb path still rejects cleanly with 503
        // instead of silently 200ing. The test name preserves the
        // "idempotent" intent — the actual idempotency lives in
        // SpiceDB and is exercised in homelab integration tests.
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit: Arc<dyn AuditLog> = Arc::new(MemoryAuditLog::default());
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        let app = router(users, Arc::new(verifier), spicedb, audit);
        let req = Request::builder()
            .method("DELETE")
            .uri("/v1/me/share/Bob")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "spicedb_unavailable");
    }

    /// Build a `ShareScope` shaped for the per-event timeline clamp
    /// tests below — kind="timeline" (so the gate doesn't 404), no
    /// window clamp (handler clamping is covered by the W3 tests),
    /// and the caller-supplied allow/deny lists piped straight through.
    fn timeline_scope(allow: Option<Vec<String>>, deny: Option<Vec<String>>) -> ShareScope {
        ShareScope {
            kind: "timeline".to_string(),
            tabs: None,
            window_days: None,
            allow_event_types: allow,
            deny_event_types: deny,
        }
    }

    /// Build a `StoredQueryEvent` for the in-memory query stub. The
    /// timeline path only inspects `claimed_handle`, `event_type`,
    /// `event_timestamp`, and `hidden_at`; the rest are filler so the
    /// tests don't carry pointless `..Default::default()` noise.
    fn evt(
        seq: i64,
        handle: &str,
        event_type: &str,
        ts: chrono::DateTime<chrono::Utc>,
    ) -> crate::repo::StoredQueryEvent {
        crate::repo::StoredQueryEvent {
            seq,
            claimed_handle: handle.to_string(),
            event_type: event_type.to_string(),
            event_timestamp: Some(ts),
            log_source: "live".into(),
            source_offset: 0,
            payload: serde_json::Value::Null,
            hidden_at: None,
        }
    }

    /// Sum every bucket in a `PublicTimelineResponse` body. The tests
    /// don't care which day a bucket landed on (the helper zero-pads
    /// over the trailing N days) — only the total surviving the
    /// per-type clamp matters for the precedence assertions.
    fn total_count(body: &serde_json::Value) -> u64 {
        body["buckets"]
            .as_array()
            .map(|arr| arr.iter().map(|b| b["count"].as_u64().unwrap_or(0)).sum())
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn timeline_scope_allow_keeps_only_listed_types() {
        // Three rows for Alice across two types, all within the
        // trailing-7d window. allow_event_types=[quantum_target_selected]
        // should drop the actor_death row and leave the two quantum rows.
        let now = chrono::Utc::now();
        let mq = crate::repo::test_support::MemoryQuery::new(vec![
            evt(1, "Alice", "quantum_target_selected", now),
            evt(2, "Alice", "quantum_target_selected", now),
            evt(3, "Alice", "actor_death", now),
        ]);
        let scope = timeline_scope(Some(vec!["quantum_target_selected".to_string()]), None);
        let resp = render_timeline_scoped(&mq, "Alice", 7, Some(&scope)).await;
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["days"], 7);
        assert_eq!(total_count(&body), 2, "allow-listed types only");
    }

    #[tokio::test]
    async fn timeline_scope_deny_excludes_listed_types() {
        // Mirror of the allow test: denylist for actor_death drops the
        // single matching row, leaving the two quantum rows.
        let now = chrono::Utc::now();
        let mq = crate::repo::test_support::MemoryQuery::new(vec![
            evt(1, "Alice", "quantum_target_selected", now),
            evt(2, "Alice", "quantum_target_selected", now),
            evt(3, "Alice", "actor_death", now),
        ]);
        let scope = timeline_scope(None, Some(vec!["actor_death".to_string()]));
        let resp = render_timeline_scoped(&mq, "Alice", 7, Some(&scope)).await;
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(total_count(&body), 2, "deny-listed type excluded");
    }

    #[tokio::test]
    async fn timeline_scope_allow_precedence_over_deny() {
        // Both lists set: allow=[quantum_target_selected] AND
        // deny=[quantum_target_selected]. Allow runs first and drops
        // anything not in its list (so actor_death is gone); the deny
        // then strips quantum out, leaving zero. This is the
        // "most restrictive wins" composition we document on the
        // ShareScope struct.
        let now = chrono::Utc::now();
        let mq = crate::repo::test_support::MemoryQuery::new(vec![
            evt(1, "Alice", "quantum_target_selected", now),
            evt(2, "Alice", "quantum_target_selected", now),
            evt(3, "Alice", "actor_death", now),
        ]);
        let scope = timeline_scope(
            Some(vec!["quantum_target_selected".to_string()]),
            Some(vec!["quantum_target_selected".to_string()]),
        );
        let resp = render_timeline_scoped(&mq, "Alice", 7, Some(&scope)).await;
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(total_count(&body), 0, "contradictory allow+deny strips all");

        // Sanity: allow alone keeps the quantum rows. Without this
        // companion assertion a regression where allow silently
        // dropped *everything* would look identical to "deny won".
        let scope_allow_only =
            timeline_scope(Some(vec!["quantum_target_selected".to_string()]), None);
        let resp2 = render_timeline_scoped(&mq, "Alice", 7, Some(&scope_allow_only)).await;
        let (_, body2) = read_body(resp2).await;
        assert_eq!(total_count(&body2), 2);
    }

    #[tokio::test]
    async fn list_shared_with_me_returns_503_when_spicedb_skipped() {
        // The inbound-shares endpoint must surface SpiceDB outages the
        // same way the outbound side does. The web client maps 503
        // -> "degraded" banner so the page still renders.
        let users = Arc::new(MemoryUserStore::new());
        let (issuer, verifier) = fresh_pair();
        let audit: Arc<dyn AuditLog> = Arc::new(MemoryAuditLog::default());
        let spicedb: Arc<Option<SpicedbClient>> = Arc::new(None);
        let (_, token) = seed_user(&users, "alice@example.com", "Alice", &issuer).await;
        let app = router(users, Arc::new(verifier), spicedb, audit);
        let req = Request::builder()
            .method("GET")
            .uri("/v1/me/shared-with-me")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let (status, body) = read_body(resp).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "spicedb_unavailable");
    }
}
