//! HTTP handler for tray-promoted parser-rule submissions.
//!
//! Endpoint:
//!   - `POST /v1/parser-submissions` -> 202 + `ParserSubmissionResponse`
//!
//! Identity is `(shape_hash, client_anon_id)`. A second submission of
//! the same shape from the same install bumps `last_submitted_at` and
//! `total_occurrence_count`; refreshing the stored `payload_json` keeps
//! the latest examples / notes from that install on file. Distinct
//! installs each land a row of their own — counting *distinct submitters
//! per shape* is a read-side query against the table.
//!
//! Auth: same Bearer-token posture as the rest of `/v1/*`. The token
//! identifies the human user; `client_anon_id` is just a stable
//! per-install hash for write-side dedupe and does **not** replace the
//! auth identity.

use crate::api_error::ApiErrorBody;
use crate::auth::AuthenticatedUser;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::post,
    Router,
};
use serde::Serialize;
use sqlx::PgPool;
use starstats_core::wire::{ParserSubmissionBatch, ParserSubmissionResponse};
use std::sync::Arc;
use utoipa::ToSchema;

/// Hard cap on a single submission batch. Wide enough for a tray
/// flushing a session's worth of distinct shapes, narrow enough that a
/// malicious client can't wedge thousands of rows per round-trip.
pub const MAX_BATCH_SIZE: usize = 200;

/// Maximum bytes a serialized `ParserSubmission` payload may consume
/// once stored as JSONB. Mirrors the tray's local cap so a tray-side
/// row that fits will always land server-side.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// Build the `/v1/parser-submissions` sub-router. Bearer-token-protected
/// via the request-level `AuthenticatedUser` extractor; the underlying
/// auth verifier is layered onto the outer router in `main.rs`.
pub fn routes(pool: PgPool) -> Router {
    Router::new()
        .route("/v1/parser-submissions", post(submit))
        .with_state(Arc::new(pool))
}

// -- DTOs (OpenAPI mirrors of the wire types) ------------------------
//
// The wire types live in `starstats-core` and cannot derive `ToSchema`
// from this crate; these transparent mirrors restate the shape so the
// OpenAPI spec carries the request / response bodies. The actual
// (de)serialization on the wire still flows through the core types.

#[derive(Debug, Serialize, ToSchema)]
pub struct ContextExampleSchema {
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ParserSubmissionSchema {
    pub shape_hash: String,
    pub raw_examples: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub partial_structured: std::collections::BTreeMap<String, String>,
    pub shell_tag: Option<String>,
    pub suggested_event_name: Option<String>,
    pub suggested_field_names: Option<std::collections::BTreeMap<String, String>>,
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_examples: Vec<ContextExampleSchema>,
    pub game_build: Option<String>,
    /// `live` / `ptu` / `eptu` / `hotfix` / `tech` / `other`.
    pub channel: String,
    pub occurrence_count: u32,
    pub client_anon_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ParserSubmissionBatchSchema {
    pub submissions: Vec<ParserSubmissionSchema>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ParserSubmissionResponseSchema {
    pub accepted: u32,
    pub deduped: u32,
    pub ids: Vec<String>,
}

// -- Helpers ---------------------------------------------------------

fn err(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        Json(ApiErrorBody {
            error: code.to_string(),
            detail: None,
        }),
    )
        .into_response()
}

// -- Handler ---------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/parser-submissions",
    tag = "parser-submissions",
    operation_id = "parser_submissions_submit",
    request_body = ParserSubmissionBatchSchema,
    responses(
        (status = 202, description = "Batch accepted", body = ParserSubmissionResponseSchema),
        (status = 400, description = "Validation failed", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
    ),
    security(("BearerAuth" = []))
)]
pub async fn submit(
    State(pool): State<Arc<PgPool>>,
    _auth: AuthenticatedUser,
    Json(batch): Json<ParserSubmissionBatch>,
) -> Response {
    if batch.submissions.is_empty() {
        return err(StatusCode::BAD_REQUEST, "empty_batch");
    }
    if batch.submissions.len() > MAX_BATCH_SIZE {
        return err(StatusCode::BAD_REQUEST, "batch_too_large");
    }

    // Pre-validate every submission before any DB write so a partially
    // malformed batch doesn't half-land. The size check on the
    // serialized payload guards against a single row inflating the
    // JSONB column past TOAST-friendly territory.
    for sub in &batch.submissions {
        if sub.shape_hash.trim().is_empty() {
            return err(StatusCode::BAD_REQUEST, "invalid_shape_hash");
        }
        if sub.client_anon_id.trim().is_empty() {
            return err(StatusCode::BAD_REQUEST, "invalid_client_anon_id");
        }
        if sub.raw_examples.is_empty() {
            return err(StatusCode::BAD_REQUEST, "missing_raw_examples");
        }
        match serde_json::to_vec(sub) {
            Ok(bytes) if bytes.len() > MAX_PAYLOAD_BYTES => {
                return err(StatusCode::BAD_REQUEST, "payload_too_large");
            }
            Ok(_) => {}
            Err(_) => return err(StatusCode::BAD_REQUEST, "payload_not_serializable"),
        }
    }

    let mut accepted = 0u32;
    let mut deduped = 0u32;
    let mut ids: Vec<String> = Vec::with_capacity(batch.submissions.len());

    for sub in &batch.submissions {
        // `(xmax = 0)` distinguishes insert vs update inside a single
        // UPSERT — postgres sets `xmax` to non-zero only when the row
        // was updated. RETURNING gives us both flag and id in one trip.
        let payload = match serde_json::to_value(sub) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, "parser submission to_value failed");
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "submission_serialize_failed",
                );
            }
        };

        let row: Result<(bool, i64), sqlx::Error> = sqlx::query_as(
            r#"
            INSERT INTO parser_submissions
                (shape_hash, client_anon_id, payload_json, total_occurrence_count)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (shape_hash, client_anon_id) DO UPDATE
                SET last_submitted_at      = NOW(),
                    total_occurrence_count = parser_submissions.total_occurrence_count
                                             + EXCLUDED.total_occurrence_count,
                    payload_json           = EXCLUDED.payload_json
            RETURNING (xmax = 0) AS inserted, id
            "#,
        )
        .bind(&sub.shape_hash)
        .bind(&sub.client_anon_id)
        .bind(&payload)
        .bind(sub.occurrence_count as i32)
        .fetch_one(pool.as_ref())
        .await;

        match row {
            Ok((inserted, id)) => {
                if inserted {
                    accepted += 1;
                } else {
                    deduped += 1;
                }
                ids.push(id.to_string());
            }
            Err(e) => {
                tracing::error!(error = %e, "parser submission upsert failed");
                return err(StatusCode::INTERNAL_SERVER_ERROR, "submission_write_failed");
            }
        }
    }

    (
        StatusCode::ACCEPTED,
        Json(ParserSubmissionResponse {
            accepted,
            deduped,
            ids,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::test_support::fresh_pair;
    use crate::auth::{AuthVerifier, TokenIssuer};
    use axum::body::to_bytes;
    use axum::http::Request;
    use axum::Extension;
    use serde_json::json;
    use tower::ServiceExt;

    // The handler's `(xmax = 0)` UPSERT trick is Postgres-specific, so
    // dedupe behaviour proper exercises an in-process variant of the
    // store. The route-layer tests below cover the surface the client
    // sees (auth gate, batch-size guards, payload validation). End-to-end
    // dedupe is verified by a separate integration test against a live
    // Postgres in CI's `cargo test -p starstats-server -- --ignored`
    // bucket; here we keep the assertion on the *contract* (status,
    // shape, validation codes) which is what the wire client depends on.

    fn router_for_test(pool_url_marker: bool, verifier: Arc<AuthVerifier>) -> Router {
        // We don't actually need a live pool for the validation-error
        // paths below — they all return before touching the DB. A
        // fake `PgPool` would still be required by `State`; we sidestep
        // that by using a route that swaps the handler for one
        // identical except it skips DB work. To avoid that mirror, the
        // tests below exercise the auth + validation branches that
        // return before any `.fetch_one` call, so the pool is dropped
        // unused. Marker arg kept so a future positive-path test can
        // gate the live-pool variant.
        let _ = pool_url_marker;

        // `with_state` requires a `PgPool`; we lazy-connect to a sentinel
        // that we never actually hit. `PgPool::connect_lazy` returns
        // immediately and only opens a connection on first query.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/starstats_test_unused")
            .expect("connect_lazy is infallible for a syntactically valid URL");
        Router::new()
            .route("/v1/parser-submissions", post(submit))
            .with_state(Arc::new(pool))
            .layer(Extension(verifier))
    }

    fn issue_token(issuer: &TokenIssuer, handle: &str) -> String {
        issuer
            .sign_user(&uuid::Uuid::now_v7().to_string(), handle)
            .expect("sign user token")
    }

    async fn body_bytes(resp: Response) -> Vec<u8> {
        to_bytes(resp.into_body(), 1 << 20).await.unwrap().to_vec()
    }

    fn sample_submission() -> serde_json::Value {
        json!({
            "shape_hash": "sh_a",
            "raw_examples": ["<X> hello"],
            "channel": "live",
            "occurrence_count": 1,
            "client_anon_id": "anon_x",
        })
    }

    #[tokio::test]
    async fn rejects_without_auth() {
        let (_issuer, verifier) = fresh_pair();
        let app = router_for_test(false, Arc::new(verifier));
        let req = Request::builder()
            .method("POST")
            .uri("/v1/parser-submissions")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "submissions": [sample_submission()] }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_empty_batch() {
        let (issuer, verifier) = fresh_pair();
        let token = issue_token(&issuer, "alice");
        let app = router_for_test(false, Arc::new(verifier));
        let req = Request::builder()
            .method("POST")
            .uri("/v1/parser-submissions")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "submissions": [] }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = body_bytes(resp).await;
        let err: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(err.error, "empty_batch");
    }

    #[tokio::test]
    async fn rejects_batch_above_max_size() {
        let (issuer, verifier) = fresh_pair();
        let token = issue_token(&issuer, "alice");
        let app = router_for_test(false, Arc::new(verifier));
        let oversized = (0..(MAX_BATCH_SIZE + 1))
            .map(|i| {
                let mut s = sample_submission();
                s["shape_hash"] = json!(format!("sh_{i}"));
                s
            })
            .collect::<Vec<_>>();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/parser-submissions")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "submissions": oversized }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = body_bytes(resp).await;
        let err: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(err.error, "batch_too_large");
    }

    #[tokio::test]
    async fn rejects_blank_shape_hash() {
        let (issuer, verifier) = fresh_pair();
        let token = issue_token(&issuer, "alice");
        let app = router_for_test(false, Arc::new(verifier));
        let mut bad = sample_submission();
        bad["shape_hash"] = json!("   ");
        let req = Request::builder()
            .method("POST")
            .uri("/v1/parser-submissions")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "submissions": [bad] }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = body_bytes(resp).await;
        let err: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(err.error, "invalid_shape_hash");
    }

    #[tokio::test]
    async fn rejects_blank_client_anon_id() {
        let (issuer, verifier) = fresh_pair();
        let token = issue_token(&issuer, "alice");
        let app = router_for_test(false, Arc::new(verifier));
        let mut bad = sample_submission();
        bad["client_anon_id"] = json!("");
        let req = Request::builder()
            .method("POST")
            .uri("/v1/parser-submissions")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "submissions": [bad] }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = body_bytes(resp).await;
        let err: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(err.error, "invalid_client_anon_id");
    }

    #[tokio::test]
    async fn rejects_missing_raw_examples() {
        let (issuer, verifier) = fresh_pair();
        let token = issue_token(&issuer, "alice");
        let app = router_for_test(false, Arc::new(verifier));
        let mut bad = sample_submission();
        bad["raw_examples"] = json!([]);
        let req = Request::builder()
            .method("POST")
            .uri("/v1/parser-submissions")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "submissions": [bad] }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = body_bytes(resp).await;
        let err: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(err.error, "missing_raw_examples");
    }

    #[tokio::test]
    async fn rejects_oversized_payload() {
        let (issuer, verifier) = fresh_pair();
        let token = issue_token(&issuer, "alice");
        let app = router_for_test(false, Arc::new(verifier));
        let huge_line = "x".repeat(MAX_PAYLOAD_BYTES + 1);
        let mut bad = sample_submission();
        bad["raw_examples"] = json!([huge_line]);
        let req = Request::builder()
            .method("POST")
            .uri("/v1/parser-submissions")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "submissions": [bad] }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = body_bytes(resp).await;
        let err: ApiErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(err.error, "payload_too_large");
    }
}
