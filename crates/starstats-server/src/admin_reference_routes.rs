//! Admin reference-data sub-router.
//!
//! Read-only inspection of the wiki-sync output (the daily cron that
//! fills `reference_registry`). Lets a moderator see which categories
//! are populated, when each was last refreshed, and browse a paged
//! list of entries within a category to spot-check the sync.
//!
//! Endpoints:
//!   GET /v1/admin/reference/categories
//!   GET /v1/admin/reference/:category
//!
//! Both gated on the moderator role. No write surface — refreshes
//! still happen via the in-tree cron; this is purely diagnostic.

use crate::admin_routes::RequireModerator;
use crate::reference_data::{ReferenceCategory, ReferenceEntry};
use crate::reference_store::{CategorySummary, ReferenceStore};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminReferenceCategoryDto {
    /// Lowercase category slug (`vehicle` / `weapon` / `item` / `location`)
    /// — matches the value used in the `category` URL segment.
    pub category: String,
    pub entry_count: i64,
    /// `MAX(updated_at)` across the rows for this category. Null when
    /// the cron hasn't populated this category yet.
    pub latest_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminReferenceCategoriesResponse {
    pub categories: Vec<AdminReferenceCategoryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminReferenceEntryDto {
    pub class_name: String,
    pub display_name: String,
    /// Free-form JSON object holding per-category extras (manufacturer,
    /// role, size, parent system…). Schema-on-read — the cron writes
    /// whatever the wiki returns and this passes it through verbatim.
    #[schema(value_type = Object)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminReferenceEntriesResponse {
    pub category: String,
    pub entries: Vec<AdminReferenceEntryDto>,
    /// Total rows in the category — same number the summary endpoint
    /// returns. Surfaced here so the UI can paginate without a second
    /// call.
    pub total: usize,
    /// Substring filter that was applied (after lowercase normalize),
    /// or null when none. Echoed back to make the URL self-describing
    /// in the admin UI.
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AdminReferenceEntriesParams {
    /// Optional case-insensitive substring filter over class_name +
    /// display_name. Applied in-memory after the store returns the
    /// full category — fine because category sizes top out around
    /// ~20k (items) and the admin tool isn't a hot path.
    #[serde(default)]
    pub q: Option<String>,
    /// Page size; defaults to 100, capped at 500. Larger than the
    /// 50/200 used elsewhere because entries are pure metadata —
    /// no SpiceDB fan-out, no JOINs — so wider pages are cheap.
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

const ENTRIES_PAGE_DEFAULT: usize = 100;
const ENTRIES_PAGE_MAX: usize = 500;

fn err_response(status: StatusCode, error: &str) -> Response {
    (status, Json(serde_json::json!({ "error": error }))).into_response()
}

fn entry_to_dto(e: ReferenceEntry) -> AdminReferenceEntryDto {
    AdminReferenceEntryDto {
        class_name: e.class_name,
        display_name: e.display_name,
        metadata: e.metadata,
    }
}

fn summary_to_dto(s: CategorySummary) -> AdminReferenceCategoryDto {
    AdminReferenceCategoryDto {
        category: s.category.as_str().to_string(),
        entry_count: s.entry_count,
        latest_updated_at: s.latest_updated_at,
    }
}

#[utoipa::path(
    get,
    path = "/v1/admin/reference/categories",
    tag = "admin",
    responses(
        (status = 200, description = "Per-category summary of reference_registry", body = AdminReferenceCategoriesResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks moderator role"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn list_reference_categories<R: ReferenceStore>(
    _: RequireModerator,
    State(refs): State<Arc<R>>,
) -> Response {
    match refs.category_summaries().await {
        Ok(rows) => (
            StatusCode::OK,
            Json(AdminReferenceCategoriesResponse {
                categories: rows.into_iter().map(summary_to_dto).collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "category_summaries failed");
            err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal")
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/admin/reference/{category}",
    tag = "admin",
    params(
        ("category" = String, Path, description = "vehicle | weapon | item | location"),
        AdminReferenceEntriesParams,
    ),
    responses(
        (status = 200, description = "Paged entry list within a category", body = AdminReferenceEntriesResponse),
        (status = 400, description = "Unknown category"),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller lacks moderator role"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn list_reference_entries<R: ReferenceStore>(
    _: RequireModerator,
    State(refs): State<Arc<R>>,
    Path(category): Path<String>,
    Query(params): Query<AdminReferenceEntriesParams>,
) -> Response {
    let Some(cat) = ReferenceCategory::parse(&category) else {
        return err_response(StatusCode::BAD_REQUEST, "unknown_category");
    };

    let entries = match refs.list_category(cat).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, category = %category, "list_category failed");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        }
    };

    // Lowercase substring filter — applied in-memory because the
    // store API is full-list-per-category. `q` is trimmed so empty
    // queries don't filter everything out.
    let needle = params
        .q
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let filtered: Vec<ReferenceEntry> = match &needle {
        Some(n) => entries
            .into_iter()
            .filter(|e| {
                e.class_name.to_lowercase().contains(n)
                    || e.display_name.to_lowercase().contains(n)
            })
            .collect(),
        None => entries,
    };

    let total = filtered.len();
    let limit = params
        .limit
        .unwrap_or(ENTRIES_PAGE_DEFAULT)
        .clamp(1, ENTRIES_PAGE_MAX);
    let offset = params.offset.unwrap_or(0);

    let page: Vec<AdminReferenceEntryDto> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(entry_to_dto)
        .collect();

    (
        StatusCode::OK,
        Json(AdminReferenceEntriesResponse {
            category: cat.as_str().to_string(),
            entries: page,
            total,
            q: needle,
        }),
    )
        .into_response()
}

pub fn router<R: ReferenceStore>(refs: Arc<R>) -> Router {
    Router::new()
        .route(
            "/v1/admin/reference/categories",
            get(list_reference_categories::<R>),
        )
        .route(
            "/v1/admin/reference/:category",
            get(list_reference_entries::<R>),
        )
        .with_state(refs)
}
