//! RSI public-profile snapshot endpoints.
//!
//! Once a user has proven ownership of their RSI handle (see
//! [`rsi_verify_routes`]), we can periodically snapshot the public
//! profile page (display name, enlistment date, location, badges,
//! short bio, primary org summary) and surface that snapshot through
//! StarStats so it can render alongside the player's stats.
//!
//! Three endpoints:
//!  - `POST /v1/auth/rsi/profile/refresh` — user-authenticated.
//!    Hits RSI, persists a fresh snapshot, returns it. Rate-limited to
//!    one refresh per hour per user; the limit is enforced inline by
//!    looking at the previous snapshot's `captured_at` because the
//!    upstream is a public HTML page that we politely shouldn't
//!    hammer.
//!  - `GET /v1/me/profile` — user-authenticated. Returns the latest
//!    stored snapshot for the caller, or 404 if none has been
//!    captured yet.
//!  - `GET /v1/public/u/{handle}/profile` — unauthenticated.
//!    Returns the latest snapshot for `handle` if (and only if) the
//!    owner has flipped public visibility on their `stats_record`.
//!    Permission resolution mirrors `sharing_routes::public_summary`:
//!    SpiceDB `view@public_view` on `stats_record:<handle>`. A failed
//!    visibility check returns 404 — never leak existence.
//!
//! All upstream-facing failure modes (RSI 404, RSI down, SpiceDB
//! down) map to the same envelope shape as the rest of the API:
//! `ApiErrorBody { error, detail }`. The `error` strings are the
//! ones the SDK + frontend already key off, so a future migration
//! away from utoipa-derived clients keeps working.

use crate::api_error::ApiErrorBody;
use crate::auth::{AuthenticatedUser, TokenType};
use crate::profile_store::{
    PostgresProfileStore, ProfileSnapshot, ProfileStore, ProfileStoreError,
};
use crate::rsi_verify::{Badge, RsiClient, RsiProfileOutcome};
use crate::spicedb::{ObjectRef, SpicedbClient};
use crate::users::{PostgresUserStore, UserStore};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer,
};
use utoipa::ToSchema;
use uuid::Uuid;

/// Minimum interval between refreshes for a single user. Set to one
/// hour: an RSI bio doesn't change minute-to-minute, and the upstream
/// is a public HTML page that we don't want to hammer. Clients that
/// hit the limit get 429 with a hint at how long to wait.
pub const PROFILE_REFRESH_COOLDOWN: chrono::Duration = chrono::Duration::hours(1);

/// Build the `/v1/auth/rsi/profile/*`, `/v1/me/profile`, and
/// `/v1/public/u/:handle/profile` sub-router.
///
/// Three internal sub-routers because the `State<_>` shape diverges:
///  - `refresh` needs both the user store (to look up the handle +
///    verification flag) and the profile store (to read the previous
///    snapshot for cooldown + write the fresh one).
///  - `me` needs only the profile store (with a `auth.sub`-derived
///    user id).
///  - `public_profile` needs both: user store to resolve the handle
///    to the live `claimed_handle` casing, profile store to fetch
///    the snapshot.
///
/// `Arc<dyn RsiClient>` and `Arc<Option<SpicedbClient>>` come in via
/// `Extension<_>` so they're shared with `rsi_verify_routes` /
/// `sharing_routes` rather than duplicated per sub-router.
pub fn routes(users: Arc<PostgresUserStore>, profiles: Arc<PostgresProfileStore>) -> Router {
    let refresh_router = Router::new()
        .route(
            "/v1/auth/rsi/profile/refresh",
            post(refresh::<PostgresUserStore, PostgresProfileStore>),
        )
        .with_state((users.clone(), profiles.clone()));

    let me_router = Router::new()
        .route("/v1/me/profile", get(me::<PostgresProfileStore>))
        .with_state(profiles.clone());

    // Per-IP throttle on the unauthenticated public endpoint. SpiceDB
    // and a DB lookup run per request, and a scanner with a list of
    // valid handles can otherwise enumerate snapshots at line rate.
    // Generous enough for normal browsing (≈1 page per 200 ms with
    // sustained 5/s + a 20-request burst).
    let public_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(5)
            .burst_size(20)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("public profile governor config builder produced no config"),
    );
    let public_router = Router::new()
        .route(
            "/v1/public/u/:handle/profile",
            get(public_profile::<PostgresUserStore, PostgresProfileStore>),
        )
        .with_state((users, profiles))
        .layer(GovernorLayer {
            config: public_governor,
        });

    refresh_router.merge(me_router).merge(public_router)
}

/// JSON shape for every successful response on these endpoints.
/// Mirrors [`ProfileSnapshot`] but trims off the internal `user_id`
/// (clients identify the profile by handle / by being authenticated)
/// and renames the field set to the wire spec the frontend expects.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProfileResponse {
    /// When this snapshot was captured. ISO-8601 UTC. Clients display
    /// this verbatim ("Last refreshed 5 minutes ago") so it's the
    /// stored timestamp, not "now" — a cached `me` response and a
    /// fresh `refresh` response with the same body must agree.
    pub captured_at: DateTime<Utc>,
    /// Display name as it appears on the RSI profile page (may differ
    /// from the user's `claimed_handle` URL slug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// "Enlisted" date from the citizen card, parsed to a `NaiveDate`
    /// because RSI publishes it without a timezone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enlistment_date: Option<chrono::NaiveDate>,
    /// Free-form location string from the profile (often a country
    /// name or "Unknown"). Surfaced verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Badges shown on the citizen card. Order is the order RSI
    /// rendered them.
    pub badges: Vec<Badge>,
    /// Short bio paragraph from the profile page. Surfaced verbatim;
    /// callers are responsible for HTML-escaping if they render it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// Primary org summary string (`"<org name> [<rank>]"` style) if
    /// the user has a non-redacted main org. Absent when the user
    /// has no main org or has marked it redacted on RSI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_org_summary: Option<String>,
}

impl From<ProfileSnapshot> for ProfileResponse {
    fn from(s: ProfileSnapshot) -> Self {
        ProfileResponse {
            captured_at: s.captured_at,
            display_name: s.display_name,
            enlistment_date: s.enlistment_date,
            location: s.location,
            badges: s.badges,
            bio: s.bio,
            primary_org_summary: s.primary_org_summary,
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn error(status: StatusCode, code: &'static str, detail: Option<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: code,
            detail,
        }),
    )
        .into_response()
}

/// Reject device tokens. The desktop client doesn't surface profile
/// management; refresh + read-me are user-account ops mirroring the
/// posture in `rsi_verify_routes`.
fn require_user_token(user: &AuthenticatedUser) -> Option<Response> {
    if !matches!(user.token_type, TokenType::User) {
        return Some(error(
            StatusCode::FORBIDDEN,
            "user_token_required",
            Some("device tokens cannot read or refresh RSI profile snapshots".into()),
        ));
    }
    None
}

use crate::users::validate_handle;

#[utoipa::path(
    post,
    path = "/v1/auth/rsi/profile/refresh",
    tag = "rsi-profile",
    operation_id = "rsi_profile_refresh",
    responses(
        (status = 200, description = "Snapshot refreshed", body = ProfileResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is a device token", body = ApiErrorBody),
        (status = 404, description = "RSI returned 404 for the claimed handle", body = ApiErrorBody),
        (status = 422, description = "Caller has not yet verified their RSI handle", body = ApiErrorBody),
        (status = 429, description = "Cooldown not elapsed; try again later", body = ApiErrorBody),
        (status = 503, description = "RSI upstream unreachable", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn refresh<U: UserStore, P: ProfileStore>(
    State((users, profiles)): State<(Arc<U>, Arc<P>)>,
    Extension(rsi): Extension<Arc<dyn RsiClient>>,
    auth: AuthenticatedUser,
) -> Response {
    if let Some(resp) = require_user_token(&auth) {
        return resp;
    }

    let user_id = match Uuid::parse_str(&auth.sub) {
        Ok(id) => id,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject", None),
    };

    let user = match users.find_by_id(user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return error(StatusCode::UNAUTHORIZED, "unauthorized", None),
        Err(e) => {
            tracing::error!(error = %e, "find_by_id failed in rsi/profile/refresh");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None);
        }
    };

    if user.rsi_verified_at.is_none() {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "rsi_handle_not_verified",
            Some(
                "verify your RSI handle (POST /v1/auth/rsi/start) before refreshing the profile"
                    .into(),
            ),
        );
    }

    // Cooldown check. We do this BEFORE hitting RSI: a hammered
    // upstream would happily 429 us, but checking locally is cheaper
    // and gives the client a clean machine-readable retry hint.
    let now = Utc::now();
    let prior = match profiles.latest_for_user(user_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "latest_for_user failed in rsi/profile/refresh");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None);
        }
    };
    if let Some(prev) = prior.as_ref() {
        let next_allowed = prev.captured_at + PROFILE_REFRESH_COOLDOWN;
        if next_allowed > now {
            let remaining = next_allowed - now;
            // Clamp to the nearest minute (rounded up) so a 1-second
            // residue still surfaces as "wait 1m" rather than "wait
            // 0m" — clients display this verbatim.
            let mins = remaining.num_seconds().div_euclid(60).max(0) + 1;
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "refresh_too_soon",
                Some(format!("wait {mins}m")),
            );
        }
    }

    match rsi.fetch_profile(&user.claimed_handle).await {
        RsiProfileOutcome::Found(profile) => {
            let snapshot = ProfileSnapshot {
                user_id,
                captured_at: now,
                display_name: profile.display_name,
                enlistment_date: profile.enlistment_date,
                location: profile.location,
                badges: profile.badges,
                bio: profile.bio,
                primary_org_summary: profile.primary_org_summary,
            };
            // `ProfileSnapshot: Clone`, so render the response from a
            // clone before handing the original to the store. Keeps
            // the response and the persisted row identical without a
            // re-fetch round trip.
            let response = ProfileResponse::from(snapshot.clone());
            if let Err(e) = profiles.save(snapshot).await {
                tracing::error!(error = %e, "profile save failed");
                return error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None);
            }
            (StatusCode::OK, Json(response)).into_response()
        }
        RsiProfileOutcome::HandleNotFound => error(
            StatusCode::NOT_FOUND,
            "rsi_handle_not_found",
            Some("RSI returned 404 for that handle".into()),
        ),
        RsiProfileOutcome::UpstreamUnavailable => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "rsi_unavailable",
            Some("RSI is unreachable; please try again shortly".into()),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/v1/me/profile",
    tag = "rsi-profile",
    operation_id = "rsi_profile_me",
    responses(
        (status = 200, description = "Latest snapshot for the caller", body = ProfileResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is a device token", body = ApiErrorBody),
        (status = 404, description = "No snapshot has been captured yet", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn me<P: ProfileStore>(
    State(profiles): State<Arc<P>>,
    auth: AuthenticatedUser,
) -> Response {
    if let Some(resp) = require_user_token(&auth) {
        return resp;
    }

    let user_id = match Uuid::parse_str(&auth.sub) {
        Ok(id) => id,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject", None),
    };

    match profiles.latest_for_user(user_id).await {
        Ok(Some(snapshot)) => {
            (StatusCode::OK, Json(ProfileResponse::from(snapshot))).into_response()
        }
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            "no_profile_yet",
            Some("call POST /v1/auth/rsi/profile/refresh to capture a snapshot".into()),
        ),
        Err(e) => {
            tracing::error!(error = %e, "latest_for_user failed in /v1/me/profile");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/public/u/{handle}/profile",
    tag = "rsi-profile",
    operation_id = "rsi_profile_public",
    params(("handle" = String, Path, description = "RSI handle to fetch the public profile snapshot for")),
    responses(
        (status = 200, description = "Latest public snapshot", body = ProfileResponse),
        (status = 404, description = "Handle unknown, not public, or no snapshot captured"),
        (status = 503, description = "SpiceDB not configured", body = ApiErrorBody),
    ),
)]
pub async fn public_profile<U: UserStore, P: ProfileStore>(
    State((users, profiles)): State<(Arc<U>, Arc<P>)>,
    Extension(spicedb): Extension<Arc<Option<SpicedbClient>>>,
    Path(handle): Path<String>,
) -> Response {
    // Same posture as `sharing_routes::public_summary`: a malformed
    // handle is indistinguishable from an unknown one, both 404.
    if !validate_handle(&handle) {
        return (StatusCode::NOT_FOUND, ()).into_response();
    }

    // Resolve the handle to a user row first. `latest_for_handle` could
    // do this in one query, but we still need to consult SpiceDB on
    // the canonical-cased handle from the user row, and rejecting
    // unknown handles before that call avoids an unnecessary
    // permission lookup.
    let user = match users.find_by_handle(&handle).await {
        Ok(Some(u)) => u,
        Ok(None) => return (StatusCode::NOT_FOUND, ()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "find_by_handle failed in public profile");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None);
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

    // Mirror `sharing_routes::check_public`: `view` on
    // `stats_record:<handle>` for the wildcard `user:*` subject.
    let resource = ObjectRef::new("stats_record", &user.claimed_handle);
    let subject = ObjectRef::new("user", "*");
    let public = match client.check_permission(resource, "view", subject).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "spicedb public profile check failed");
            return error(StatusCode::SERVICE_UNAVAILABLE, "spicedb_unavailable", None);
        }
    };
    if !public {
        return (StatusCode::NOT_FOUND, ()).into_response();
    }

    match profiles.latest_for_handle(&user.claimed_handle).await {
        Ok(Some(snapshot)) => {
            (StatusCode::OK, Json(ProfileResponse::from(snapshot))).into_response()
        }
        // Same 404 as "not public" — don't disclose to anonymous
        // callers that a public user simply hasn't refreshed yet.
        Ok(None) => (StatusCode::NOT_FOUND, ()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "latest_for_handle failed in public profile");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

// `ProfileStoreError` is referenced via `tracing::error!` interpolation
// above; importing the type keeps the `%e` Display impl resolution
// honest and is also needed for the bin's openapi build (the `mod`
// declaration mirrors the live server's so unused imports are
// suppressed via `#![allow(unused_imports)]` at the bin root).
#[allow(dead_code)]
type _UnusedProfileStoreError = ProfileStoreError;
