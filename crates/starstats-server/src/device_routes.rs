//! Device pairing endpoints.
//!
//!  - `POST /v1/auth/devices/start`  (user JWT required) — mints a
//!    short-lived pairing code the user reads to the desktop client.
//!  - `POST /v1/auth/devices/redeem` (no auth) — desktop client
//!    submits the code, receives a long-lived device JWT.
//!
//! The redeem endpoint is intentionally unauthenticated: the desktop
//! client is being paired *because* it doesn't yet hold a token.
//! Authorisation comes from possession of the code, which only the
//! authenticated web session could have generated.

use crate::api_error::ApiErrorBody;
use crate::auth::{AuthenticatedUser, TokenIssuer, TokenType};
use crate::devices::{DeviceError, DeviceStore, PostgresDeviceStore, PAIRING_TTL};
use crate::users::{PostgresUserStore, UserStore};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use uuid::Uuid;

/// Build the `/v1/auth/devices/*` sub-router.
///
/// Three sub-routers internally because each set of endpoints needs a
/// different `State<_>` shape: `start` only needs the device store,
/// `list`/`revoke` likewise (but as a different router for clarity),
/// and `redeem` needs both device + user stores so it gets its own.
pub fn routes(devices: Arc<PostgresDeviceStore>, users: Arc<PostgresUserStore>) -> Router {
    let start_router = Router::new()
        .route("/v1/auth/devices/start", post(start::<PostgresDeviceStore>))
        .with_state(devices.clone());

    let list_router = Router::new()
        .route("/v1/auth/devices", get(list::<PostgresDeviceStore>))
        .route(
            "/v1/auth/devices/:id",
            delete(revoke::<PostgresDeviceStore>),
        )
        .with_state(devices.clone());

    let redeem_router = Router::new()
        .route(
            "/v1/auth/devices/redeem",
            post(redeem::<PostgresDeviceStore, PostgresUserStore>),
        )
        .with_state((devices, users));

    start_router.merge(list_router).merge(redeem_router)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct StartRequest {
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StartResponse {
    pub code: String,
    /// ISO-8601 UTC. Clients show this so the user knows how long
    /// they have to type the code into the desktop client.
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub label: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RedeemRequest {
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RedeemResponse {
    pub token: String,
    pub device_id: String,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeviceDto {
    pub id: String,
    pub label: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    detail: Option<String>,
}

/// Used by the auth extractor: only `User` tokens may mint pairings.
/// A device should not be able to bootstrap further devices.
fn require_user_token(user: &AuthenticatedUser) -> Option<axum::response::Response> {
    if !matches!(user.token_type, TokenType::User) {
        return Some(error(
            StatusCode::FORBIDDEN,
            "user_token_required",
            Some("device tokens cannot generate pairings".into()),
        ));
    }
    None
}

#[utoipa::path(
    post,
    path = "/v1/auth/devices/start",
    tag = "devices",
    request_body = StartRequest,
    responses(
        (status = 200, description = "Pairing code minted", body = StartResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is a device token, not a user", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn start<D: DeviceStore>(
    State(devices): State<Arc<D>>,
    user: AuthenticatedUser,
    Json(req): Json<StartRequest>,
) -> impl IntoResponse {
    if let Some(resp) = require_user_token(&user) {
        return resp;
    }

    let user_id = match Uuid::parse_str(&user.sub) {
        Ok(id) => id,
        Err(_) => {
            tracing::error!(sub = %user.sub, "user JWT sub is not a UUID");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject", None);
        }
    };

    let label = req.label.unwrap_or_default();
    match devices.create_pairing(user_id, &label, PAIRING_TTL).await {
        Ok(pairing) => (
            StatusCode::OK,
            Json(StartResponse {
                code: pairing.code,
                expires_at: pairing.expires_at,
                label: pairing.label,
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "create pairing failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

/// Slice 3's redeem endpoint is generic over the device + user
/// stores so it can be wired against memory impls in tests and
/// Postgres impls in production.
#[utoipa::path(
    post,
    path = "/v1/auth/devices/redeem",
    tag = "devices",
    request_body = RedeemRequest,
    responses(
        (status = 200, description = "Pairing redeemed; device token returned", body = RedeemResponse),
        (status = 400, description = "Malformed code", body = ApiErrorBody),
        (status = 404, description = "Unknown code", body = ApiErrorBody),
        (status = 409, description = "Code already redeemed", body = ApiErrorBody),
        (status = 410, description = "Code expired", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    )
)]
pub async fn redeem<D: DeviceStore, U: UserStore>(
    State((devices, users)): State<(Arc<D>, Arc<U>)>,
    Extension(issuer): Extension<Arc<TokenIssuer>>,
    Json(req): Json<RedeemRequest>,
) -> impl IntoResponse {
    let code = req.code.trim().to_uppercase();
    if code.len() != 8 {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_code",
            Some("pairing codes are 8 characters".into()),
        );
    }

    let redeemed = match devices.redeem(&code).await {
        Ok(d) => d,
        Err(DeviceError::UnknownCode) => return error(StatusCode::NOT_FOUND, "unknown_code", None),
        Err(DeviceError::AlreadyRedeemed) => {
            return error(StatusCode::CONFLICT, "already_redeemed", None)
        }
        Err(DeviceError::Expired) => return error(StatusCode::GONE, "expired", None),
        Err(e) => {
            tracing::error!(error = %e, "redeem pairing failed");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None);
        }
    };

    // Look up the user's claimed_handle so we can stamp it into the
    // device JWT's preferred_username — keeps the ingest cross-check
    // (`claimed_handle == preferred_username`) working unchanged.
    let user = match users.find_by_id(redeemed.user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            tracing::error!(
                user_id = %redeemed.user_id,
                "user disappeared between redeem and lookup"
            );
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None);
        }
        Err(e) => {
            tracing::error!(error = %e, "user lookup failed during redeem");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None);
        }
    };

    match issuer.sign_device(
        &user.id.to_string(),
        &user.claimed_handle,
        redeemed.device_id,
    ) {
        Ok(token) => (
            StatusCode::OK,
            Json(RedeemResponse {
                token,
                device_id: redeemed.device_id.to_string(),
                label: redeemed.label,
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "sign device token failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "sign_failed", None)
        }
    }
}

// -- list + revoke ----------------------------------------------------

#[utoipa::path(
    get,
    path = "/v1/auth/devices",
    tag = "devices",
    responses(
        (status = 200, description = "Active devices for the caller", body = DeviceListResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is a device token", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn list<D: DeviceStore>(
    State(devices): State<Arc<D>>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    if let Some(resp) = require_user_token(&user) {
        return resp;
    }
    let user_id = match Uuid::parse_str(&user.sub) {
        Ok(id) => id,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject", None),
    };
    match devices.list_for_user(user_id).await {
        Ok(rows) => (
            StatusCode::OK,
            Json(DeviceListResponse {
                devices: rows
                    .into_iter()
                    .map(|d| DeviceDto {
                        id: d.id.to_string(),
                        label: d.label,
                        created_at: d.created_at,
                        last_seen_at: d.last_seen_at,
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "list devices failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/v1/auth/devices/{id}",
    tag = "devices",
    params(
        ("id" = String, Path, description = "Device UUID")
    ),
    responses(
        (status = 204, description = "Device revoked"),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is a device token", body = ApiErrorBody),
        (status = 404, description = "Device not found for caller", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn revoke<D: DeviceStore>(
    State(devices): State<Arc<D>>,
    user: AuthenticatedUser,
    Path(device_id): Path<Uuid>,
) -> impl IntoResponse {
    if let Some(resp) = require_user_token(&user) {
        return resp;
    }
    let user_id = match Uuid::parse_str(&user.sub) {
        Ok(id) => id,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject", None),
    };
    match devices.revoke(user_id, device_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(DeviceError::DeviceNotFound) => error(StatusCode::NOT_FOUND, "device_not_found", None),
        Err(e) => {
            tracing::error!(error = %e, "revoke device failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

fn error(
    status: StatusCode,
    code: &'static str,
    detail: Option<String>,
) -> axum::response::Response {
    (
        status,
        Json(ErrorBody {
            error: code,
            detail,
        }),
    )
        .into_response()
}

// -- tests -----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::test_support::fresh_pair;
    use crate::auth::AuthVerifier;
    use crate::devices::test_support::MemoryDeviceStore;
    use crate::users::test_support::MemoryUserStore;
    use crate::users::{hash_password, UserStore};
    use axum::body::to_bytes;
    use axum::http::Request;
    use axum::routing::post;
    use axum::Router;
    use tower::ServiceExt;

    /// Build the redeem-side router (the only one we exercise via
    /// HTTP — `start` is hit by calling the handler directly so we
    /// can inject an `AuthenticatedUser` without bearer plumbing).
    /// Returns the router, the matching verifier so we can decode
    /// the device JWT, and the seeded fixtures.
    async fn fixture() -> (
        Router,
        AuthVerifier,
        Arc<MemoryUserStore>,
        Arc<MemoryDeviceStore>,
        crate::users::User,
    ) {
        let users = Arc::new(MemoryUserStore::new());
        let devices = Arc::new(MemoryDeviceStore::new());
        let phc = hash_password("supersecret-1234").unwrap();
        let user = users
            .create("daisy@example.com", &phc, "TheCodeSaiyan")
            .await
            .unwrap();

        let (issuer, verifier) = fresh_pair();
        let issuer_arc = Arc::new(issuer);

        let app = Router::new()
            .route(
                "/v1/auth/devices/redeem",
                post(redeem::<MemoryDeviceStore, MemoryUserStore>),
            )
            .layer(Extension(issuer_arc))
            .with_state((devices.clone(), users.clone()));

        (app, verifier, users, devices, user)
    }

    async fn post_json<B: serde::Serialize>(
        app: &Router,
        path: &str,
        body: &B,
        bearer: Option<&str>,
    ) -> (StatusCode, axum::body::Bytes) {
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(t) = bearer {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let req = req
            .body(axum::body::Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, bytes)
    }

    /// Tests for `start` bypass the FromRequestParts extractor by
    /// invoking the handler directly with a synthetic
    /// `AuthenticatedUser` — no bearer token plumbing required.
    /// The redeem path goes through the full router because that
    /// endpoint takes no auth.
    #[tokio::test]
    async fn start_emits_pairing_for_authenticated_user() {
        let (_, _, _, devices, user) = fixture().await;
        let auth = AuthenticatedUser {
            sub: user.id.to_string(),
            preferred_username: user.claimed_handle.clone(),
            token_type: TokenType::User,
            device_id: None,
        };
        let resp = start(
            State(devices.clone()),
            auth,
            Json(StartRequest {
                label: Some("Daisy's PC".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let parsed: StartResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.label, "Daisy's PC");
        assert_eq!(parsed.code.len(), 8);
    }

    #[tokio::test]
    async fn start_rejects_device_token() {
        let (_, _, _, devices, user) = fixture().await;
        let auth = AuthenticatedUser {
            sub: user.id.to_string(),
            preferred_username: user.claimed_handle.clone(),
            token_type: TokenType::Device,
            device_id: Some(Uuid::new_v4()),
        };
        let resp = start(
            State(devices.clone()),
            auth,
            Json(StartRequest { label: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn redeem_returns_device_token_with_user_handle() {
        let (app, verifier, users, devices, user) = fixture().await;
        // Create a pairing directly via the store for this test.
        let pairing = devices
            .create_pairing(user.id, "Daisy's PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        // redeem hits the wired route (no bearer required)
        let body = serde_json::json!({"code": pairing.code});
        let (status, bytes) = post_json(&app, "/v1/auth/devices/redeem", &body, None).await;
        assert_eq!(status, StatusCode::OK);
        let parsed: RedeemResponse = serde_json::from_slice(&bytes).unwrap();
        let claims = verifier.verify(&parsed.token).unwrap();
        assert_eq!(claims.token_type, TokenType::Device);
        assert_eq!(claims.preferred_username, "TheCodeSaiyan");
        assert_eq!(claims.sub, user.id.to_string());
        // Sanity: parameters we want for ingest cross-check.
        let _ = users;
    }

    #[tokio::test]
    async fn redeem_rejects_already_used_code() {
        let (app, _, _, devices, user) = fixture().await;
        let pairing = devices
            .create_pairing(user.id, "Daisy's PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        let body = serde_json::json!({"code": pairing.code});
        let (s1, _) = post_json(&app, "/v1/auth/devices/redeem", &body, None).await;
        assert_eq!(s1, StatusCode::OK);
        let (s2, _) = post_json(&app, "/v1/auth/devices/redeem", &body, None).await;
        assert_eq!(s2, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn redeem_rejects_unknown_code() {
        let (app, _, _, _, _) = fixture().await;
        let body = serde_json::json!({"code": "ZZZZZZZZ"});
        let (status, _) = post_json(&app, "/v1/auth/devices/redeem", &body, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn redeem_rejects_expired_code() {
        let (app, _, _, devices, user) = fixture().await;
        let pairing = devices
            .create_pairing(user.id, "Daisy's PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        devices.expire_now(&pairing.code);
        let body = serde_json::json!({"code": pairing.code});
        let (status, _) = post_json(&app, "/v1/auth/devices/redeem", &body, None).await;
        assert_eq!(status, StatusCode::GONE);
    }

    #[tokio::test]
    async fn redeem_rejects_malformed_code() {
        let (app, _, _, _, _) = fixture().await;
        let body = serde_json::json!({"code": "TOO-SHORT"});
        let (status, _) = post_json(&app, "/v1/auth/devices/redeem", &body, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_returns_active_devices_for_caller() {
        let (_, _, _, devices, user) = fixture().await;
        let other_user_id = Uuid::new_v4();
        let p1 = devices
            .create_pairing(user.id, "PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        let r1 = devices.redeem(&p1.code).await.unwrap();
        let p2 = devices
            .create_pairing(user.id, "Laptop", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        let r2 = devices.redeem(&p2.code).await.unwrap();
        let p3 = devices
            .create_pairing(other_user_id, "Their PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        let _ = devices.redeem(&p3.code).await.unwrap();

        // Revoke r2 so it shouldn't appear in the list.
        devices.revoke(user.id, r2.device_id).await.unwrap();

        let auth = AuthenticatedUser {
            sub: user.id.to_string(),
            preferred_username: user.claimed_handle.clone(),
            token_type: TokenType::User,
            device_id: None,
        };
        let resp = list(State(devices.clone()), auth).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let parsed: DeviceListResponse = serde_json::from_slice(&bytes).unwrap();
        // Only r1 — r2 revoked, r3 belongs to another user.
        assert_eq!(parsed.devices.len(), 1);
        assert_eq!(parsed.devices[0].id, r1.device_id.to_string());
    }

    #[tokio::test]
    async fn revoke_invalidates_device() {
        let (_, _, _, devices, user) = fixture().await;
        let p1 = devices
            .create_pairing(user.id, "PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        let r1 = devices.redeem(&p1.code).await.unwrap();
        assert!(devices.is_device_active(r1.device_id).await.unwrap());

        let auth = AuthenticatedUser {
            sub: user.id.to_string(),
            preferred_username: user.claimed_handle.clone(),
            token_type: TokenType::User,
            device_id: None,
        };
        let resp = revoke(
            State(devices.clone()),
            auth,
            axum::extract::Path(r1.device_id),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(!devices.is_device_active(r1.device_id).await.unwrap());
    }

    /// End-to-end revocation enforcement: a device JWT that worked
    /// against a protected endpoint stops working the instant its
    /// devices row is revoked.
    #[tokio::test]
    async fn extractor_rejects_token_after_revoke() {
        use crate::auth::{AuthVerifier, TokenIssuer};
        use axum::routing::get;

        // Tiny protected endpoint that just echoes the authenticated
        // user. Lets us hit the extractor end-to-end.
        async fn protected(_user: AuthenticatedUser) -> StatusCode {
            StatusCode::OK
        }

        let users = Arc::new(MemoryUserStore::new());
        let devices_arc = Arc::new(MemoryDeviceStore::new());
        let phc = hash_password("supersecret-1234").unwrap();
        let user = users
            .create("daisy@example.com", &phc, "TheCodeSaiyan")
            .await
            .unwrap();

        let (issuer, verifier): (TokenIssuer, AuthVerifier) = fresh_pair();

        // Mint a device + a device JWT pointing at it.
        let pairing = devices_arc
            .create_pairing(user.id, "PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        let redeemed = devices_arc.redeem(&pairing.code).await.unwrap();
        let token = issuer
            .sign_device(
                &user.id.to_string(),
                &user.claimed_handle,
                redeemed.device_id,
            )
            .unwrap();

        // Wire the protected endpoint with both Extensions.
        let device_store_dyn: Arc<dyn DeviceStore> = devices_arc.clone();
        let app = Router::new()
            .route("/protected", get(protected))
            .layer(Extension(Arc::new(verifier)))
            .layer(Extension(device_store_dyn));

        // Before revoke: 200.
        let req = Request::builder()
            .method("GET")
            .uri("/protected")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Revoke the device.
        devices_arc
            .revoke(user.id, redeemed.device_id)
            .await
            .unwrap();

        // Same token, after revoke: 401.
        let req = Request::builder()
            .method("GET")
            .uri("/protected")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn revoke_404_for_other_users_device() {
        let (_, _, _, devices, user) = fixture().await;
        let other_user_id = Uuid::new_v4();
        let p = devices
            .create_pairing(other_user_id, "Their PC", crate::devices::PAIRING_TTL)
            .await
            .unwrap();
        let r = devices.redeem(&p.code).await.unwrap();

        let auth = AuthenticatedUser {
            sub: user.id.to_string(),
            preferred_username: user.claimed_handle.clone(),
            token_type: TokenType::User,
            device_id: None,
        };
        let resp = revoke(
            State(devices.clone()),
            auth,
            axum::extract::Path(r.device_id),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // Owner's device is unaffected.
        assert!(devices.is_device_active(r.device_id).await.unwrap());
    }
}
