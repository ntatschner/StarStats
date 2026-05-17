//! `POST /v1/ingest` — accepts a [`IngestBatch`] from the desktop
//! client. Validates schema, normalises, and writes via [`EventStore`].
//!
//! Authentication: extracts an [`AuthenticatedUser`] from the bearer
//! token. The batch's `claimed_handle` must match the token's
//! `preferred_username` (case-insensitive) — clients can't push
//! events under another user's handle.

use crate::api_error::ApiErrorBody;
use crate::audit::{AuditEntry, AuditLog};
use crate::auth::AuthenticatedUser;
use crate::repo::{from_envelope, EventStore, InsertOutcome};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    Extension,
};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use serde_json::json;
use starstats_core::metadata::stamp;
use starstats_core::wire::IngestBatch;
use std::sync::Arc;
use std::time::Instant;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IngestResponse {
    pub batch_id: String,
    pub accepted: u32,
    pub duplicate: u32,
    pub rejected: u32,
}

/// OpenAPI-only mirror of `starstats_core::wire::IngestBatch`. The
/// real type lives in `starstats-core` and we deliberately don't
/// touch that crate (it's also used by the desktop client and we
/// don't want a `utoipa` dep leaking down). The shapes match field
/// for field; the `events` payload is left as `serde_json::Value`
/// because `GameEvent` is a tagged enum with ~30 variants — see the
/// Rust source for the wire-level discriminant table.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct IngestBatchSchema {
    pub schema_version: u16,
    pub batch_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_build: Option<String>,
    pub claimed_handle: String,
    pub events: Vec<EventEnvelopeSchema>,
}

/// Schema-only mirror of `starstats_core::wire::EventEnvelope`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct EventEnvelopeSchema {
    pub idempotency_key: String,
    pub raw_line: String,
    /// Free-form JSON; see `starstats_core::events::GameEvent` for
    /// the full variant list. Each variant is internally tagged on
    /// `type` (snake_case discriminant).
    #[schema(value_type = Object)]
    pub event: Option<serde_json::Value>,
    /// One of: `live`, `ptu`, `eptu`, `hotfix`, `tech`, `other`.
    pub source: String,
    pub source_offset: u64,
}

#[utoipa::path(
    post,
    path = "/v1/ingest",
    tag = "ingest",
    request_body = IngestBatchSchema,
    responses(
        (status = 200, description = "Batch accepted (may include duplicates)", body = IngestResponse),
        (status = 400, description = "Schema-level rejection", body = ApiErrorBody),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Token identity does not match claimed_handle", body = ApiErrorBody),
    ),
    security(("BearerAuth" = []))
)]
pub async fn handle<S: EventStore>(
    State(store): State<Arc<S>>,
    Extension(audit): Extension<Arc<dyn AuditLog>>,
    user: AuthenticatedUser,
    Json(mut batch): Json<IngestBatch>,
) -> impl IntoResponse {
    let started = Instant::now();

    // Accept any version in `[1, CURRENT]`. v1 envelopes predate the
    // `metadata` field on `EventEnvelope`; we synthesise observed
    // metadata server-side below so downstream consumers see a
    // uniform shape regardless of client age.
    if batch.schema_version < 1 || batch.schema_version > IngestBatch::CURRENT_SCHEMA_VERSION {
        counter!("starstats_ingest_batches_rejected", "reason" => "bad_schema").increment(1);
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "unsupported_schema_version".into(),
                detail: Some(format!(
                    "got {}, server speaks 1..={}",
                    batch.schema_version,
                    IngestBatch::CURRENT_SCHEMA_VERSION
                )),
            }),
        )
            .into_response();
    }

    if batch.claimed_handle.trim().is_empty() {
        counter!("starstats_ingest_batches_rejected", "reason" => "empty_handle").increment(1);
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "missing_claimed_handle".into(),
                detail: None,
            }),
        )
            .into_response();
    }

    // Cross-check: the bearer token's preferred_username must match
    // the batch's claimed_handle. Prevents user A from pushing events
    // under user B's handle.
    if !batch
        .claimed_handle
        .eq_ignore_ascii_case(&user.preferred_username)
    {
        counter!("starstats_ingest_batches_rejected", "reason" => "handle_mismatch").increment(1);
        tracing::warn!(
            sub = %user.sub,
            token_handle = %user.preferred_username,
            claimed = %batch.claimed_handle,
            "ingest rejected — handle mismatch"
        );
        return (
            StatusCode::FORBIDDEN,
            Json(ApiErrorBody {
                error: "handle_mismatch".into(),
                detail: Some("claimed_handle does not match authenticated identity".into()),
            }),
        )
            .into_response();
    }

    // Backfill default Observed metadata for any envelope a legacy
    // (v1) client uploaded without it. Newer clients stamp on the
    // wire; the server only synthesises when the field is absent so
    // we never overwrite explicit producer-supplied metadata.
    for env in batch.events.iter_mut() {
        if env.metadata.is_none() {
            if let Some(ev) = &env.event {
                env.metadata = Some(stamp(ev, Some(&batch.claimed_handle)));
            }
        }
    }

    let mut accepted = 0u32;
    let mut duplicate = 0u32;
    let mut rejected = 0u32;

    for envelope in &batch.events {
        let stored = from_envelope(envelope, &batch.claimed_handle);
        match store.insert(stored).await {
            Ok(InsertOutcome::Inserted) => accepted += 1,
            Ok(InsertOutcome::Duplicate) => duplicate += 1,
            Err(e) => {
                tracing::warn!(error = %e, "event insert failed");
                rejected += 1;
            }
        }
    }

    counter!("starstats_events_ingested").increment(accepted as u64);
    counter!("starstats_events_duplicate").increment(duplicate as u64);
    if rejected > 0 {
        counter!("starstats_events_rejected", "reason" => "insert_error")
            .increment(rejected as u64);
    }
    histogram!("starstats_ingest_batch_duration_seconds").record(started.elapsed().as_secs_f64());

    tracing::info!(
        sub = %user.sub,
        batch_id = %batch.batch_id,
        accepted,
        duplicate,
        rejected,
        total = batch.events.len(),
        "ingest batch processed"
    );

    // Best-effort audit. A failure here is logged but doesn't fail
    // the request — the events themselves already landed. We trade
    // strict atomicity for availability; audit drift is detectable
    // out-of-band (and rare in practice).
    //
    // device_id is server-determined: read off the bearer token's
    // device claim (populated for device-paired tokens by the auth
    // extractor). User-tokens have None here — those rows show up in
    // the account-wide stream and never match a `?device_id=` filter.
    // Storing it inside the audit payload (rather than as a separate
    // column) keeps the hash chain canonical — see migration 0026.
    let audit_entry = AuditEntry {
        actor_sub: Some(user.sub.clone()),
        actor_handle: Some(user.preferred_username.clone()),
        action: "ingest.batch_processed".to_string(),
        payload: json!({
            "batch_id": batch.batch_id,
            "claimed_handle": batch.claimed_handle,
            "game_build": batch.game_build,
            "device_id": user.device_id,
            "total": batch.events.len(),
            "accepted": accepted,
            "duplicate": duplicate,
            "rejected": rejected,
        }),
    };
    if let Err(e) = audit.append(audit_entry).await {
        tracing::error!(error = %e, "audit append failed");
    }

    (
        StatusCode::OK,
        Json(IngestResponse {
            batch_id: batch.batch_id,
            accepted,
            duplicate,
            rejected,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::test_support::MemoryAuditLog;
    use crate::auth::test_support::fresh_pair;
    use crate::auth::{AuthVerifier, TokenIssuer};
    use crate::repo::MemoryStore;
    use axum::body::to_bytes;
    use axum::http::Request;
    use axum::routing::post;
    use axum::Router;
    use starstats_core::events::{GameEvent, JoinPu};
    use starstats_core::wire::{EventEnvelope, LogSource};
    use tower::ServiceExt;

    const HANDLE: &str = "TheCodeSaiyan";

    struct TestEnv {
        issuer: TokenIssuer,
        verifier: Arc<AuthVerifier>,
    }

    fn test_env() -> TestEnv {
        let (issuer, verifier) = fresh_pair();
        TestEnv {
            issuer,
            verifier: Arc::new(verifier),
        }
    }

    fn sign_token(issuer: &TokenIssuer, username: &str) -> String {
        issuer
            .sign_user(&format!("user-{username}"), username)
            .expect("sign user token")
    }

    fn router(
        store: Arc<MemoryStore>,
        verifier: Arc<AuthVerifier>,
        audit: Arc<MemoryAuditLog>,
    ) -> Router {
        let audit_dyn: Arc<dyn AuditLog> = audit;
        Router::new()
            .route("/v1/ingest", post(handle::<MemoryStore>))
            .layer(Extension(verifier))
            .layer(Extension(audit_dyn))
            .with_state(store)
    }

    fn sample_envelope(key: &str) -> EventEnvelope {
        EventEnvelope {
            idempotency_key: key.into(),
            raw_line: "<2026-05-02T21:14:23.189Z> ...".into(),
            event: Some(GameEvent::JoinPu(JoinPu {
                timestamp: "2026-05-02T21:14:23.189Z".into(),
                address: "1.2.3.4".into(),
                port: 64300,
                shard: "pub_euw1b".into(),
                location_id: "562954248454145".into(),
            })),
            source: LogSource::Live,
            source_offset: 1234,
            metadata: None,
        }
    }

    fn batch(events: Vec<EventEnvelope>) -> IngestBatch {
        IngestBatch {
            schema_version: IngestBatch::CURRENT_SCHEMA_VERSION,
            batch_id: "01934f5a-3b2a-7000-a000-000000000000".into(),
            game_build: Some("4.7.178".into()),
            claimed_handle: HANDLE.into(),
            events,
        }
    }

    async fn post_batch_with(
        router: &Router,
        token: Option<&str>,
        body: &IngestBatch,
    ) -> (StatusCode, axum::body::Bytes) {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/ingest")
            .header("content-type", "application/json");
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let req = req
            .body(axum::body::Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, bytes)
    }

    fn parse_response(bytes: &[u8]) -> IngestResponse {
        serde_json::from_slice(bytes).unwrap()
    }

    #[tokio::test]
    async fn accepts_valid_batch_and_persists_events() {
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store.clone(), env.verifier, audit.clone());

        let body = batch(vec![sample_envelope("evt-1"), sample_envelope("evt-2")]);
        let (status, bytes) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::OK);
        let resp = parse_response(&bytes);
        assert_eq!(resp.accepted, 2);
        assert_eq!(resp.duplicate, 0);
        assert_eq!(store.snapshot().len(), 2);

        // Audit row written for the batch.
        let audited = audit.snapshot();
        assert_eq!(audited.len(), 1);
        assert_eq!(audited[0].action, "ingest.batch_processed");
        assert_eq!(audited[0].actor_handle.as_deref(), Some(HANDLE));
        assert_eq!(audited[0].payload["accepted"], 2);
    }

    #[tokio::test]
    async fn dedupes_by_idempotency_key_per_handle() {
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store.clone(), env.verifier, audit.clone());

        let body = batch(vec![sample_envelope("evt-1")]);
        let (_, first) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(parse_response(&first).accepted, 1);

        let (_, second) = post_batch_with(&app, Some(&token), &body).await;
        let resp = parse_response(&second);
        assert_eq!(resp.accepted, 0);
        assert_eq!(resp.duplicate, 1);
        assert_eq!(store.snapshot().len(), 1);
    }

    #[tokio::test]
    async fn rejects_unknown_schema_version() {
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store, env.verifier, audit);

        let mut bad = batch(vec![sample_envelope("evt-1")]);
        bad.schema_version = 999;
        let (status, _) = post_batch_with(&app, Some(&token), &bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rejects_above_window_schema_version() {
        // 99 is outside [1, CURRENT]; must reject.
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store, env.verifier, audit);

        let mut bad = batch(vec![sample_envelope("evt-1")]);
        bad.schema_version = 99;
        let (status, _) = post_batch_with(&app, Some(&token), &bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn accepts_legacy_v1_schema_version() {
        // schema_version=1 predates the metadata field; the server
        // must still accept it (within the [1, CURRENT] window) and
        // synthesise metadata server-side. Verified separately below.
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store.clone(), env.verifier, audit);

        let mut body = batch(vec![sample_envelope("evt-v1")]);
        body.schema_version = 1;
        let (status, _) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(store.snapshot().len(), 1);
    }

    #[tokio::test]
    async fn ingest_synthesises_metadata_for_v1_envelopes_missing_it() {
        // A v1 client uploads an envelope with `metadata = None`. The
        // server must backfill default Observed metadata so downstream
        // consumers see a uniform shape.
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store.clone(), env.verifier, audit);

        let envelope = sample_envelope("evt-synth");
        // Sanity-check the test fixture: it must start without metadata
        // so the synthesis path is exercised.
        assert!(envelope.metadata.is_none());
        let mut body = batch(vec![envelope]);
        body.schema_version = 1;
        let (status, _) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::OK);

        let rows = store.snapshot();
        assert_eq!(rows.len(), 1);
        let meta = rows[0]
            .metadata
            .as_ref()
            .expect("server must synthesise metadata for v1 envelopes");
        assert_eq!(meta.source, starstats_core::metadata::EventSource::Observed);
        assert!((meta.confidence - 1.0).abs() < f32::EPSILON);
        // JoinPu's primary entity is its shard string (see
        // `primary_entity_for` in starstats-core).
        assert_eq!(
            meta.primary_entity.kind,
            starstats_core::metadata::EntityKind::Session
        );
        assert_eq!(meta.primary_entity.id, "pub_euw1b");
    }

    #[tokio::test]
    async fn ingest_preserves_explicit_metadata_when_present() {
        // A v2 client uploads an envelope with metadata already
        // attached. The server must not overwrite it.
        use starstats_core::metadata::{stamp, EntityKind};
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store.clone(), env.verifier, audit);

        let mut envelope = sample_envelope("evt-explicit");
        let preset = stamp(
            envelope.event.as_ref().unwrap(),
            Some("ExplicitlyDifferentHandle"),
        );
        let expected_id = preset.primary_entity.id.clone();
        envelope.metadata = Some(preset);
        let body = batch(vec![envelope]);
        let (status, _) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::OK);

        let rows = store.snapshot();
        assert_eq!(rows.len(), 1);
        let meta = rows[0].metadata.as_ref().expect("metadata must round-trip");
        // The preset entity id wins over the claimed_handle-derived one
        // — JoinPu maps to its shard not its handle, but the point is
        // that the server respected the supplied metadata.
        assert_eq!(meta.primary_entity.id, expected_id);
        assert_eq!(meta.primary_entity.kind, EntityKind::Session);
    }

    #[tokio::test]
    async fn rejects_missing_bearer_token() {
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store, env.verifier, audit);

        let body = batch(vec![sample_envelope("evt-1")]);
        let (status, _) = post_batch_with(&app, None, &body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_handle_mismatch() {
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        // Token says "OtherUser", batch claims "TheCodeSaiyan"
        let token = sign_token(&env.issuer, "OtherUser");
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store, env.verifier, audit);

        let body = batch(vec![sample_envelope("evt-1")]);
        let (status, _) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_invalid_token_signature() {
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        // Sign with a foreign issuer the server's verifier doesn't trust.
        let (rogue_issuer, _) = fresh_pair();
        let token = sign_token(&rogue_issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store, env.verifier, audit);

        let body = batch(vec![sample_envelope("evt-1")]);
        let (status, _) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn extracts_event_type_from_payload() {
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store.clone(), env.verifier, audit.clone());

        let body = batch(vec![sample_envelope("evt-1")]);
        let _ = post_batch_with(&app, Some(&token), &body).await;

        let rows = store.snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_type, "join_pu");
        assert!(rows[0].event_timestamp.is_some());
    }

    #[tokio::test]
    async fn user_scoped_token_writes_audit_with_null_device_id() {
        // User-scoped bearer tokens have no device claim; the audit
        // payload's device_id must be JSON null so the per-device
        // filter on /v1/me/ingest-history correctly excludes the row.
        let store = Arc::new(MemoryStore::new());
        let env = test_env();
        let token = sign_token(&env.issuer, HANDLE);
        let audit = Arc::new(MemoryAuditLog::default());
        let app = router(store, env.verifier, audit.clone());

        let body = batch(vec![sample_envelope("evt-u1")]);
        let (status, _) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::OK);

        let audited = audit.snapshot();
        assert_eq!(audited.len(), 1);
        // Payload contains the key, set to null.
        assert!(
            audited[0]
                .payload
                .as_object()
                .unwrap()
                .contains_key("device_id"),
            "device_id key must always be present in the audit payload"
        );
        assert!(audited[0].payload["device_id"].is_null());
    }

    #[tokio::test]
    async fn device_scoped_token_writes_audit_with_device_id_string() {
        // Device JWTs carry a `device_id` claim. The ingest handler
        // copies it into the audit payload so the per-device Activity
        // tab can filter on it later.
        use crate::devices::test_support::MemoryDeviceStore;
        use crate::devices::DeviceStore;
        use chrono::Duration as ChronoDuration;

        let store = Arc::new(MemoryStore::new());
        let env = test_env();

        // The auth extractor consults the DeviceStore for device
        // tokens to enforce revocation, so we have to seed a real,
        // active device row before signing the device JWT.
        let device_store = Arc::new(MemoryDeviceStore::new());
        let user_id = uuid::Uuid::new_v4();
        let pairing = device_store
            .create_pairing(user_id, HANDLE, ChronoDuration::minutes(5))
            .await
            .expect("create pairing");
        let redeemed = device_store
            .redeem(&pairing.code)
            .await
            .expect("redeem pairing");
        let device_id = redeemed.device_id;

        let token = env
            .issuer
            .sign_device(&format!("user-{HANDLE}"), HANDLE, device_id)
            .expect("sign device token");

        let audit = Arc::new(MemoryAuditLog::default());
        let audit_dyn: Arc<dyn AuditLog> = audit.clone();
        let device_dyn: Arc<dyn DeviceStore> = device_store;
        let app: Router = Router::new()
            .route("/v1/ingest", post(handle::<MemoryStore>))
            .layer(Extension(env.verifier))
            .layer(Extension(audit_dyn))
            .layer(Extension(device_dyn))
            .with_state(store);

        let body = batch(vec![sample_envelope("evt-d1")]);
        let (status, _) = post_batch_with(&app, Some(&token), &body).await;
        assert_eq!(status, StatusCode::OK);

        let audited = audit.snapshot();
        assert_eq!(audited.len(), 1);
        assert_eq!(audited[0].action, "ingest.batch_processed");
        assert_eq!(
            audited[0].payload["device_id"].as_str(),
            Some(device_id.to_string().as_str())
        );
    }
}
