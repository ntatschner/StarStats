//! Per-user UI preferences endpoints.
//!
//! `GET /v1/me/preferences` returns the caller's stored preferences
//! (or an empty `UserPreferences` payload when nothing has been
//! persisted yet — clients should treat absent fields as "use the
//! default" rather than 404'ing on first load).
//!
//! `PUT /v1/me/preferences` replaces the stored payload in full. The
//! body is a `UserPreferences` and is validated server-side: the
//! current allowlist for `theme` is `{stanton, pyro, terra, nyx}`.
//! Unknown themes return 400 `invalid_theme` rather than silently
//! storing — keeping the JSONB column from accumulating typos.
//!
//! Both endpoints are user-token only, mirroring the posture of
//! `hangar_routes` and `rsi_org_routes`. Device tokens cannot read or
//! write preferences (preferences belong to a person, not a paired
//! tray client).

use crate::api_error::ApiErrorBody;
use crate::auth::{AuthenticatedUser, TokenType};
use crate::preferences_store::PreferencesStore;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer,
};
use utoipa::ToSchema;
use uuid::Uuid;

/// 1 KB hard cap on the PUT body. The current schema only carries a
/// short `theme` string; even with the full forward-extensible field
/// set (theme + a handful of toggles + a 28-char name plate) the
/// payload comfortably fits in a few hundred bytes. The 1 KB ceiling
/// bounds the parse cost without rejecting any realistic client.
const MAX_BODY_BYTES: usize = 1024;

/// Theme allowlist — must match the four front-end themes the
/// frontend ships. The Postgres column is unconstrained JSONB
/// (deliberately, see migration 0015) so the gate lives here. Order
/// is alphabetical for stable error messages; the set is small enough
/// that linear scan is faster than a HashSet.
const ALLOWED_THEMES: &[&str] = &["nyx", "pyro", "stanton", "terra"];

// Schema-only mirror of `starstats_core::wire::UserPreferences`. The
// real type lives in `starstats-core`, which has no `utoipa` dep —
// same pattern as `hangar_routes::HangarPushRequestSchema`. Keep this
// in sync with the core type; drift here silently breaks the OpenAPI
// clients without a compile error.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UserPreferencesSchema {
    /// Active theme. One of `stanton`, `pyro`, `terra`, `nyx`.
    /// Optional so a fresh account (no preferences set) round-trips
    /// as `{}` rather than forcing a default into storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

/// Build the `/v1/me/preferences` sub-router. Per-IP rate limited
/// (1/s sustained, burst 5) — matches `hangar_routes`. Realistic
/// usage is one PUT per theme switch (rare) and one GET per page
/// load, so the limit is generous for legitimate traffic while
/// bounding a runaway client.
pub fn routes<S: PreferencesStore>(store: Arc<S>) -> Router {
    let governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(1)
            .burst_size(5)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("preferences governor config builder produced no config"),
    );
    Router::new()
        .route("/v1/me/preferences", routing::get(get::<S>).put(put::<S>))
        .with_state(store)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(GovernorLayer { config: governor })
}

fn error(status: StatusCode, code: &'static str, detail: Option<String>) -> Response {
    (
        status,
        Json(ApiErrorBody {
            error: code.to_string(),
            detail,
        }),
    )
        .into_response()
}

/// Reject device tokens. Mirrors `hangar_routes::require_user_token`.
fn require_user_token(user: &AuthenticatedUser) -> Option<Response> {
    if !matches!(user.token_type, TokenType::User) {
        return Some(error(
            StatusCode::FORBIDDEN,
            "user_token_required",
            Some("device tokens cannot read or write user preferences".into()),
        ));
    }
    None
}

#[utoipa::path(
    get,
    path = "/v1/me/preferences",
    tag = "preferences",
    operation_id = "preferences_get",
    responses(
        (status = 200, description = "Stored preferences (empty object when none set)", body = UserPreferencesSchema),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is a device token", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn get<S: PreferencesStore>(
    State(store): State<Arc<S>>,
    auth: AuthenticatedUser,
) -> Response {
    if let Some(resp) = require_user_token(&auth) {
        return resp;
    }

    let user_id = match Uuid::parse_str(&auth.sub) {
        Ok(id) => id,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject", None),
    };

    match store.get(user_id).await {
        Ok(prefs) => (StatusCode::OK, Json(prefs)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "preferences get failed in /v1/me/preferences");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

#[utoipa::path(
    put,
    path = "/v1/me/preferences",
    tag = "preferences",
    operation_id = "preferences_put",
    request_body = UserPreferencesSchema,
    responses(
        (status = 200, description = "Preferences stored", body = UserPreferencesSchema),
        (status = 400, description = "Invalid theme or malformed body", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is a device token", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn put<S: PreferencesStore>(
    State(store): State<Arc<S>>,
    auth: AuthenticatedUser,
    Json(body): Json<starstats_core::wire::UserPreferences>,
) -> Response {
    if let Some(resp) = require_user_token(&auth) {
        return resp;
    }

    if let Some(theme) = body.theme.as_deref() {
        if !ALLOWED_THEMES.contains(&theme) {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_theme",
                Some(format!(
                    "theme must be one of {ALLOWED_THEMES:?}; got {theme:?}"
                )),
            );
        }
    }

    let user_id = match Uuid::parse_str(&auth.sub) {
        Ok(id) => id,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "bad_subject", None),
    };

    match store.put(user_id, &body).await {
        Ok(()) => (StatusCode::OK, Json(body)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "preferences put failed in /v1/me/preferences");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences_store::test_support::MemoryPreferencesStore;
    use starstats_core::wire::UserPreferences;

    /// Direct exercise of the validation + store behaviour — the
    /// AuthenticatedUser extractor is a bearer-token-driven trait, so
    /// the cheapest unit-level coverage walks the same paths the
    /// handler does after auth resolves.

    #[tokio::test]
    async fn get_defaults_to_empty_when_nothing_stored() {
        let store = MemoryPreferencesStore::new();
        let user = Uuid::new_v4();

        let prefs = store.get(user).await.unwrap();
        assert!(prefs.theme.is_none());
    }

    #[tokio::test]
    async fn put_round_trips_valid_theme() {
        let store = MemoryPreferencesStore::new();
        let user = Uuid::new_v4();

        for theme in ALLOWED_THEMES {
            let prefs = UserPreferences {
                theme: Some((*theme).to_string()),
            };
            store.put(user, &prefs).await.unwrap();
            let got = store.get(user).await.unwrap();
            assert_eq!(got.theme.as_deref(), Some(*theme));
        }
    }

    #[tokio::test]
    async fn invalid_theme_is_rejected_by_allowlist() {
        // The handler rejects unknown themes with 400 `invalid_theme`
        // before ever touching the store. Recreate the gate inline so
        // this test is hermetic (no axum runtime needed).
        let bad_themes = ["", "STANTON", "Stanton", "microtech", "pyrö", " "];
        for t in bad_themes {
            assert!(
                !ALLOWED_THEMES.contains(&t),
                "{t:?} unexpectedly in allowlist"
            );
        }

        // And confirm the four real themes do pass.
        for t in ["stanton", "pyro", "terra", "nyx"] {
            assert!(ALLOWED_THEMES.contains(&t), "{t:?} missing from allowlist");
        }
    }

    #[tokio::test]
    async fn empty_preferences_round_trip() {
        // A PUT with no fields set must round-trip as an empty object,
        // not error out — this is the "reset to defaults" path.
        let store = MemoryPreferencesStore::new();
        let user = Uuid::new_v4();

        store
            .put(
                user,
                &UserPreferences {
                    theme: Some("pyro".into()),
                },
            )
            .await
            .unwrap();
        store.put(user, &UserPreferences::default()).await.unwrap();

        let got = store.get(user).await.unwrap();
        assert_eq!(got, UserPreferences::default());
    }
}
