//! Admin users sub-router.
//!
//! Gated on the moderator role for read operations and the admin
//! role for role grants/revokes (mirrors how staff escalation works
//! in most systems — moderators can investigate, admins can promote).
//!
//! Endpoints:
//!   GET    /v1/admin/users
//!   GET    /v1/admin/users/:id
//!   POST   /v1/admin/users/:id/roles
//!   DELETE /v1/admin/users/:id/roles/:role
//!
//! Audit trail: every grant/revoke writes one `staff.grant` or
//! `staff.revoke` row via the existing audit log. The grant/revoke
//! routes are intentionally idempotent — replaying a grant for an
//! already-active role returns 200 with `changed: false` instead of
//! erroring, which keeps retry-on-network-blip from surfacing as
//! "Forbidden / already a moderator" to the admin UI.

use crate::admin_routes::{RequireAdmin, RequireModerator};
use crate::audit::{AuditEntry, AuditLog};
use crate::staff_roles::{StaffRole, StaffRoleStore};
use crate::users::{ListUsersFilters, User, UserStore};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

// -- DTOs -----------------------------------------------------------------

/// Lightweight admin-side view of a user. Skips secrets (password
/// hash, TOTP secret) and verification tokens. Keeps only the bits
/// an admin needs to triage an account.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminUserDto {
    pub id: Uuid,
    pub email: String,
    pub claimed_handle: String,
    pub created_at: DateTime<Utc>,
    pub email_verified: bool,
    pub rsi_verified: bool,
    pub totp_enabled: bool,
    /// Active staff roles (e.g. `["moderator"]`, `["admin"]`).
    /// Always present; empty array for ordinary users.
    pub staff_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminUserListResponse {
    pub users: Vec<AdminUserDto>,
    pub has_more: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AdminUserListParams {
    /// Substring search against `claimed_handle` OR `email`.
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct GrantRoleRequest {
    pub role: String,
    /// Optional free-text note for the audit trail (e.g.
    /// "promoted at quarterly review"). Capped at 280 chars by the
    /// handler; longer values rejected with 400.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RoleTransitionResponse {
    /// Whether the call actually changed state. `false` for a grant
    /// against an already-active role or a revoke against an
    /// already-inactive role.
    pub changed: bool,
    /// The user's active role set after the operation. Lets the UI
    /// re-render without a follow-up GET.
    pub staff_roles: Vec<String>,
}

const USERS_PAGE_DEFAULT: i64 = 50;
const USERS_PAGE_MAX: i64 = 200;
const REASON_MAX_LEN: usize = 280;

fn err_body(error: &str) -> serde_json::Value {
    serde_json::json!({ "error": error })
}

fn err_response(status: StatusCode, error: &str) -> Response {
    (status, Json(err_body(error))).into_response()
}

/// Materialise an `AdminUserDto` from a `User` plus its active
/// staff-role set.
async fn to_dto(user: User, staff: &Arc<dyn StaffRoleStore>) -> Result<AdminUserDto, Response> {
    let roles = match staff.list_active_for_user(user.id).await {
        Ok(r) => r.as_strings(),
        Err(e) => {
            tracing::error!(error = %e, user_id = %user.id, "staff_roles lookup failed");
            return Err(err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "staff_roles_lookup_failed",
            ));
        }
    };
    Ok(AdminUserDto {
        id: user.id,
        email: user.email,
        claimed_handle: user.claimed_handle,
        created_at: user.created_at,
        email_verified: user.email_verified_at.is_some(),
        rsi_verified: user.rsi_verified_at.is_some(),
        totp_enabled: user.totp_enabled_at.is_some(),
        staff_roles: roles,
    })
}

// -- Handlers -------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/v1/admin/users",
    tag = "admin",
    params(AdminUserListParams),
    responses(
        (status = 200, description = "Users page (most recent first)", body = AdminUserListResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks moderator role"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn list_users_admin<U: UserStore>(
    _: RequireModerator,
    State(users): State<Arc<U>>,
    Extension(staff): Extension<Arc<dyn StaffRoleStore>>,
    Query(params): Query<AdminUserListParams>,
) -> Response {
    let limit = params
        .limit
        .unwrap_or(USERS_PAGE_DEFAULT)
        .clamp(1, USERS_PAGE_MAX);
    let offset = params.offset.unwrap_or(0).max(0);

    let filters = ListUsersFilters {
        q: params
            .q
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        limit,
        offset,
    };

    let user_rows = match users.list_users(filters).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "list_users failed");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    };

    let has_more = user_rows.len() as i64 >= limit;
    let mut dtos = Vec::with_capacity(user_rows.len());
    for user in user_rows {
        match to_dto(user, &staff).await {
            Ok(dto) => dtos.push(dto),
            Err(resp) => return resp,
        }
    }
    (
        StatusCode::OK,
        Json(AdminUserListResponse {
            users: dtos,
            has_more,
        }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/v1/admin/users/{id}",
    tag = "admin",
    params(("id" = String, Path, description = "User UUID")),
    responses(
        (status = 200, description = "User detail", body = AdminUserDto),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks moderator role"),
        (status = 404, description = "User not found"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn get_user_admin<U: UserStore>(
    _: RequireModerator,
    State(users): State<Arc<U>>,
    Extension(staff): Extension<Arc<dyn StaffRoleStore>>,
    Path(id_str): Path<String>,
) -> Response {
    let Ok(id) = Uuid::parse_str(&id_str) else {
        return err_response(StatusCode::NOT_FOUND, "user_not_found");
    };
    let user = match users.find_by_id(id).await {
        Ok(Some(u)) => u,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "user_not_found"),
        Err(e) => {
            tracing::error!(error = %e, "find_by_id failed");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    };
    match to_dto(user, &staff).await {
        Ok(dto) => (StatusCode::OK, Json(dto)).into_response(),
        Err(resp) => resp,
    }
}

#[utoipa::path(
    post,
    path = "/v1/admin/users/{id}/roles",
    tag = "admin",
    request_body = GrantRoleRequest,
    params(("id" = String, Path, description = "Target user UUID")),
    responses(
        (status = 200, description = "Role granted (idempotent)", body = RoleTransitionResponse),
        (status = 400, description = "Invalid role or reason"),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks admin role"),
        (status = 404, description = "Target user not found"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn grant_role<U: UserStore>(
    actor: RequireAdmin,
    State(users): State<Arc<U>>,
    Extension(staff): Extension<Arc<dyn StaffRoleStore>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    Path(id_str): Path<String>,
    Json(req): Json<GrantRoleRequest>,
) -> Response {
    let Ok(target_id) = Uuid::parse_str(&id_str) else {
        return err_response(StatusCode::NOT_FOUND, "user_not_found");
    };
    let Ok(role) = req.role.parse::<StaffRole>() else {
        return err_response(StatusCode::BAD_REQUEST, "invalid_role");
    };
    if let Some(r) = req.reason.as_ref() {
        if r.chars().count() > REASON_MAX_LEN {
            return err_response(StatusCode::BAD_REQUEST, "reason_too_long");
        }
    }

    match users.find_by_id(target_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "user_not_found"),
        Err(e) => {
            tracing::error!(error = %e, "find_by_id failed in grant_role");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    }

    let Ok(actor_id) = Uuid::parse_str(&actor.0.sub) else {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject");
    };
    let reason_str = req
        .reason
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let changed = match staff
        .grant(target_id, role, Some(actor_id), reason_str)
        .await
    {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "staff grant failed");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "staff_store_error");
        }
    };

    if changed {
        if let Err(e) = audit
            .append(AuditEntry {
                actor_sub: Some(actor.0.sub.clone()),
                actor_handle: Some(actor.0.preferred_username.clone()),
                action: "staff.grant".to_string(),
                payload: serde_json::json!({
                    "target_user_id": target_id,
                    "role": role.as_str(),
                    "reason": reason_str,
                }),
            })
            .await
        {
            tracing::warn!(error = %e, "audit append (staff.grant) failed");
        }
    }

    let roles = match staff.list_active_for_user(target_id).await {
        Ok(r) => r.as_strings(),
        Err(e) => {
            tracing::error!(error = %e, "staff list after grant failed");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "staff_store_error");
        }
    };

    (
        StatusCode::OK,
        Json(RoleTransitionResponse {
            changed,
            staff_roles: roles,
        }),
    )
        .into_response()
}

#[utoipa::path(
    delete,
    path = "/v1/admin/users/{id}/roles/{role}",
    tag = "admin",
    params(
        ("id" = String, Path, description = "Target user UUID"),
        ("role" = String, Path, description = "Role to revoke (moderator|admin)"),
    ),
    responses(
        (status = 200, description = "Role revoked (idempotent)", body = RoleTransitionResponse),
        (status = 400, description = "Invalid role"),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks admin role"),
        (status = 404, description = "Target user not found"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn revoke_role<U: UserStore>(
    actor: RequireAdmin,
    State(users): State<Arc<U>>,
    Extension(staff): Extension<Arc<dyn StaffRoleStore>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    Path((id_str, role_str)): Path<(String, String)>,
) -> Response {
    let Ok(target_id) = Uuid::parse_str(&id_str) else {
        return err_response(StatusCode::NOT_FOUND, "user_not_found");
    };
    let Ok(role) = role_str.parse::<StaffRole>() else {
        return err_response(StatusCode::BAD_REQUEST, "invalid_role");
    };

    // Don't let an admin revoke their own admin role — they'd lock
    // themselves out, and the UI's "are you sure" guard is best
    // mirrored server-side too.
    if let Ok(actor_id) = Uuid::parse_str(&actor.0.sub) {
        if actor_id == target_id && role == StaffRole::Admin {
            return err_response(StatusCode::BAD_REQUEST, "cannot_revoke_own_admin");
        }
    }

    match users.find_by_id(target_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "user_not_found"),
        Err(e) => {
            tracing::error!(error = %e, "find_by_id failed in revoke_role");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    }

    let actor_id = Uuid::parse_str(&actor.0.sub).ok();
    let changed = match staff.revoke(target_id, role, actor_id).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "staff revoke failed");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "staff_store_error");
        }
    };

    if changed {
        if let Err(e) = audit
            .append(AuditEntry {
                actor_sub: Some(actor.0.sub.clone()),
                actor_handle: Some(actor.0.preferred_username.clone()),
                action: "staff.revoke".to_string(),
                payload: serde_json::json!({
                    "target_user_id": target_id,
                    "role": role.as_str(),
                }),
            })
            .await
        {
            tracing::warn!(error = %e, "audit append (staff.revoke) failed");
        }
    }

    let roles = match staff.list_active_for_user(target_id).await {
        Ok(r) => r.as_strings(),
        Err(e) => {
            tracing::error!(error = %e, "staff list after revoke failed");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "staff_store_error");
        }
    };

    (
        StatusCode::OK,
        Json(RoleTransitionResponse {
            changed,
            staff_roles: roles,
        }),
    )
        .into_response()
}

pub fn router<U: UserStore>(users: Arc<U>) -> Router {
    Router::new()
        .route("/v1/admin/users", get(list_users_admin::<U>))
        .route("/v1/admin/users/:id", get(get_user_admin::<U>))
        .route("/v1/admin/users/:id/roles", post(grant_role::<U>))
        .route("/v1/admin/users/:id/roles/:role", delete(revoke_role::<U>))
        .with_state(users)
}
