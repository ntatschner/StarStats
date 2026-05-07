//! Public, read-only endpoint that hosts the runtime parser-definition
//! manifest fetched by the tray client.
//!
//! `GET /v1/parser-definitions` returns a [`Manifest`] from
//! `starstats-core`. v1 ships a hardcoded empty manifest — the fetch
//! plumbing exists end-to-end but no rules are published yet. When
//! we have rules to ship, the source flips to a config file or
//! object-storage backend without touching the wire shape.
//!
//! Rate-limited per-IP to discourage scraping. The response is
//! freshness-tolerant: clients cache for hours, so a 429 here is a
//! non-event.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use starstats_core::Manifest;
use std::sync::Arc;
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer,
};
use utoipa::ToSchema;

/// Build the `/v1/parser-definitions` sub-router. Unauthenticated;
/// IP-rate-limited.
pub fn routes() -> Router {
    let governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(2)
            .burst_size(10)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("parser-defs governor config builder produced no config"),
    );
    Router::new()
        .route("/v1/parser-definitions", get(get_manifest))
        .layer(GovernorLayer { config: governor })
}

/// OpenAPI-friendly wrapper. utoipa can't derive `ToSchema` for the
/// `starstats_core::Manifest` directly because it lives in another
/// crate; this transparent wrapper restates the shape minimally.
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ManifestResponse {
    pub version: u32,
    pub schema_version: u32,
    pub issued_at: String,
    pub rules: Vec<RemoteRuleDoc>,
    pub signature: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RemoteRuleDoc {
    pub id: String,
    pub event_name: String,
    pub match_kind: String,
    pub body_regex: String,
    pub fields: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/v1/parser-definitions",
    tag = "parser-definitions",
    operation_id = "parser_definitions_get_manifest",
    responses(
        (status = 200, description = "Active parser-definition manifest", body = ManifestResponse),
    ),
)]
pub async fn get_manifest() -> Response {
    let manifest = current_manifest();
    (StatusCode::OK, Json(manifest)).into_response()
}

/// Source-of-truth for the active manifest. Hardcoded for v1; flip to
/// a config-file load (or an object-storage fetch) when we start
/// publishing real rules. Keep this function pure so a future async
/// fetch can replace its body without churning the route handler.
fn current_manifest() -> Manifest {
    Manifest {
        version: 1,
        schema_version: 1,
        // Stable issued_at for v1 — every empty manifest is the same
        // manifest; clients should not refetch hourly while the
        // version is unchanged.
        issued_at: "2026-05-07T00:00:00Z".to_string(),
        rules: Vec::new(),
        signature: None,
    }
}
