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
use crate::reference_data::VehicleReference;
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
    Router::new()
        .route(
            "/v1/reference/vehicles",
            get(list_vehicles::<PostgresReferenceStore>),
        )
        .route(
            "/v1/reference/vehicles/:class_name",
            get(get_vehicle::<PostgresReferenceStore>),
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
