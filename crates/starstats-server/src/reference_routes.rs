//! Public read-only endpoints over the cached vehicle/item reference
//! data sourced from `api.star-citizen.wiki`.
//!
//! Two endpoints, both unauthenticated:
//!  - `GET /v1/reference/vehicles` — full list. Clients (the dashboard
//!    in particular) preload this once on first render and key
//!    everything else off `class_name`.
//!  - `GET /v1/reference/vehicles/:class_name` — single lookup.
//!    `class_name` matching is case-insensitive: the in-game prop
//!    names ("ANVL_Hornet_F7C", "anvl_hornet_f7c") differ in casing
//!    across data sources, and forcing clients to remember the
//!    canonical form would be a footgun.
//!
//! Rate-limited per-IP (10/s with a burst of 40) — generous enough
//! that the dashboard's "preload" hit doesn't trip the limiter on
//! cold-load even with the user clicking around fast, but tight
//! enough that a scraper can't pull the full list multiple times
//! per second without slowing down. The data is freshness-tolerant
//! (refreshed once per 24h server-side), so a 429 here is a
//! non-event for normal callers.

use crate::api_error::ApiErrorBody;
use crate::reference_data::{ReferenceCategory, ReferenceEntry, VehicleReference};
use crate::reference_store::{PostgresReferenceStore, ReferenceStore};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer,
};
use utoipa::ToSchema;

/// Build the `/v1/reference/*` sub-router. Public — no `BearerAuth`
/// layer is attached, but the per-IP rate limit is.
pub fn routes(store: Arc<PostgresReferenceStore>) -> Router {
    let public_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(10)
            .burst_size(40)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("reference governor config builder produced no config"),
    );
    // Axum's matchit router prefers static segments over wildcards
    // when both could match the same path, so the legacy
    // `/v1/reference/vehicles` route wins over the generic
    // `/v1/reference/:category` route for the literal "vehicles"
    // segment. Registration order is informational only.
    Router::new()
        .route(
            "/v1/reference/vehicles",
            get(list_vehicles::<PostgresReferenceStore>),
        )
        .route(
            "/v1/reference/vehicles/:class_name",
            get(get_vehicle::<PostgresReferenceStore>),
        )
        .route(
            "/v1/reference/:category",
            get(list_entries::<PostgresReferenceStore>),
        )
        .route(
            "/v1/reference/:category/:class_name",
            get(get_entry::<PostgresReferenceStore>),
        )
        .with_state(store)
        .layer(GovernorLayer {
            config: public_governor,
        })
}

/// Wrapper for `GET /v1/reference/vehicles`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VehicleListResponse {
    pub vehicles: Vec<VehicleReference>,
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

#[utoipa::path(
    get,
    path = "/v1/reference/vehicles",
    tag = "reference",
    operation_id = "reference_list_vehicles",
    responses(
        (status = 200, description = "Full list of cached vehicles", body = VehicleListResponse),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
)]
pub async fn list_vehicles<R: ReferenceStore>(State(store): State<Arc<R>>) -> Response {
    match store.list_vehicles().await {
        Ok(vehicles) => (StatusCode::OK, Json(VehicleListResponse { vehicles })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "list_vehicles failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/reference/vehicles/{class_name}",
    tag = "reference",
    operation_id = "reference_get_vehicle",
    params(("class_name" = String, Path, description = "Vehicle class_name (case-insensitive)")),
    responses(
        (status = 200, description = "Vehicle entry", body = VehicleReference),
        (status = 404, description = "No vehicle with that class_name", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
)]
pub async fn get_vehicle<R: ReferenceStore>(
    State(store): State<Arc<R>>,
    Path(class_name): Path<String>,
) -> Response {
    // The store contract (see `ReferenceStore::get_vehicle`) guarantees
    // case-insensitive lookup, matching the `lower(class_name)` index.
    // The route is a thin pass-through — no fallback scan needed.
    match store.get_vehicle(&class_name).await {
        Ok(Some(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "vehicle_not_found", None),
        Err(e) => {
            tracing::error!(error = %e, "get_vehicle failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

/// Response wrapper for `GET /v1/reference/{category}`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ReferenceListResponse {
    pub entries: Vec<ReferenceEntry>,
}

#[utoipa::path(
    get,
    path = "/v1/reference/{category}",
    tag = "reference",
    operation_id = "reference_list_category",
    params(("category" = String, Path, description = "One of: vehicle, weapon, item, location")),
    responses(
        (status = 200, description = "Full list of cached entries for the category", body = ReferenceListResponse),
        (status = 404, description = "Unknown category", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
)]
pub async fn list_entries<R: ReferenceStore>(
    State(store): State<Arc<R>>,
    Path(category): Path<String>,
) -> Response {
    // Categories outside the allow-list 404 — the alternative is
    // letting the DB hit return an empty list, which would mask
    // typos like `/v1/reference/vehciles`.
    let Some(cat) = ReferenceCategory::parse(&category) else {
        return error(
            StatusCode::NOT_FOUND,
            "unknown_category",
            Some(format!(
                "category '{category}' is not recognised; expected one of: vehicle, weapon, item, location"
            )),
        );
    };
    match store.list_category(cat).await {
        Ok(entries) => (StatusCode::OK, Json(ReferenceListResponse { entries })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, category = %category, "list_entries failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/reference/{category}/{class_name}",
    tag = "reference",
    operation_id = "reference_get_entry",
    params(
        ("category" = String, Path, description = "One of: vehicle, weapon, item, location"),
        ("class_name" = String, Path, description = "Entry class_name (case-insensitive)"),
    ),
    responses(
        (status = 200, description = "Reference entry", body = ReferenceEntry),
        (status = 404, description = "No entry with that (category, class_name)", body = ApiErrorBody),
        (status = 500, description = "Server error", body = ApiErrorBody),
    ),
)]
pub async fn get_entry<R: ReferenceStore>(
    State(store): State<Arc<R>>,
    Path((category, class_name)): Path<(String, String)>,
) -> Response {
    let Some(cat) = ReferenceCategory::parse(&category) else {
        return error(
            StatusCode::NOT_FOUND,
            "unknown_category",
            Some(format!(
                "category '{category}' is not recognised; expected one of: vehicle, weapon, item, location"
            )),
        );
    };
    match store.get_entry(cat, &class_name).await {
        Ok(Some(entry)) => (StatusCode::OK, Json(entry)).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "entry_not_found", None),
        Err(e) => {
            tracing::error!(error = %e, category = %category, "get_entry failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
        }
    }
}
