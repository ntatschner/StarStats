//! StarStats API server bootstrap.
//!
//! Wires `/healthz`, `/readyz`, `/metrics` for ops, `/v1/ingest` for
//! the desktop client, `/v1/me/*` for read queries. Self-hosted JWT
//! auth — the server loads or generates an RSA keypair at startup and
//! mints + verifies its own tokens.

use crate::audit::{AuditLog, PostgresAuditLog};
use crate::audit_mirror::MinioMirror;
use crate::auth::{AuthVerifier, ServerKey, TokenIssuer};
use crate::config::Config;
use crate::devices::PostgresDeviceStore;
use crate::hangar_store::PostgresHangarStore;
use crate::kek::Kek;
use crate::magic_link::PostgresMagicLinkStore;
use crate::mail::Mailer;
use crate::orgs::PostgresOrgStore;
use crate::preferences_store::PostgresPreferencesStore;
use crate::profile_store::PostgresProfileStore;
use crate::recovery_codes::PostgresRecoveryCodeStore;
use crate::reference_data::{ReferenceCategory, ReferenceClient, ReferenceFetchOutcomeCategory};
use crate::reference_store::ReferenceStore;
use crate::repo::PostgresStore;
use crate::rsi_org_store::PostgresRsiOrgStore;
use crate::spicedb::SpicedbClient;
use crate::staff_roles::{PostgresStaffRoleStore, StaffRoleStore};
use crate::telemetry::{init_telemetry, TelemetryHandles};
use crate::users::PostgresUserStore;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Extension, Router,
};
use sqlx::postgres::PgPoolOptions;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

mod admin_routes;
mod admin_submission_routes;
mod api_error;
mod audit;
mod audit_mirror;
mod auth;
mod auth_routes;
mod config;
mod device_routes;
mod devices;
mod hangar_routes;
mod hangar_store;
mod health;
mod ingest;
mod kek;
mod locations;
mod magic_link;
mod magic_link_routes;
mod mail;
mod openapi;
mod orders;
mod org_routes;
mod orgs;
mod parser_def_routes;
mod preferences_routes;
mod preferences_store;
mod profile_store;
mod query;
mod recovery_codes;
mod reference_data;
mod reference_routes;
mod reference_store;
mod repo;
mod revolut;
mod revolut_routes;
mod rsi_org_routes;
mod rsi_org_store;
mod rsi_profile_routes;
mod rsi_verify;
mod rsi_verify_routes;
mod sharing_routes;
mod smtp_admin_routes;
mod smtp_config_store;
mod spicedb;
mod staff_roles;
mod submission_routes;
mod submissions;
mod supporter_routes;
mod supporters;
mod telemetry;
mod totp;
mod totp_routes;
mod update_routes;
mod users;
mod validation;
mod well_known;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let TelemetryHandles {
        prometheus,
        otel_guard,
    } = init_telemetry()?;

    let cfg = Config::from_env()?;

    let pool = PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let store = Arc::new(PostgresStore::new(pool.clone()));
    let users = Arc::new(PostgresUserStore::new(pool.clone()));
    let reference_store = Arc::new(reference_store::PostgresReferenceStore::new(pool.clone()));
    let reference_client = Arc::new(reference_data::WikiReferenceClient::new()?);
    let profiles: Arc<PostgresProfileStore> = Arc::new(PostgresProfileStore::new(pool.clone()));
    let orgs: Arc<PostgresOrgStore> = Arc::new(PostgresOrgStore::new(pool.clone()));
    let rsi_orgs: Arc<PostgresRsiOrgStore> = Arc::new(PostgresRsiOrgStore::new(pool.clone()));
    let health_pool = pool.clone();
    let devices: Arc<PostgresDeviceStore> = Arc::new(PostgresDeviceStore::new(pool.clone()));
    let hangars: Arc<PostgresHangarStore> = Arc::new(PostgresHangarStore::new(pool.clone()));
    let preferences: Arc<PostgresPreferencesStore> =
        Arc::new(PostgresPreferencesStore::new(pool.clone()));
    // The auth extractor consults this dyn handle on every device-token
    // request to enforce revocation.
    let device_store_dyn: Arc<dyn devices::DeviceStore> = devices.clone();

    // Connect to SpiceDB if configured. Same posture as the OTel
    // exporter: a missing or unreachable sidecar logs a warning and
    // boots in degraded mode rather than failing.
    let spicedb: Arc<Option<SpicedbClient>> = match cfg.spicedb.clone() {
        Some(sc) => match SpicedbClient::connect(sc).await {
            Ok(c) => {
                tracing::info!("SpiceDB client connected");
                Arc::new(Some(c))
            }
            Err(e) => {
                tracing::warn!(error = %e, "SpiceDB connect failed; continuing without authz client");
                Arc::new(None)
            }
        },
        None => {
            tracing::info!("SpiceDB not configured (no preshared key); skipping");
            Arc::new(None)
        }
    };

    // Connect to MinIO if configured. Same posture as SpiceDB: missing
    // credentials -> skipped; unreachable bucket -> warn-and-degrade.
    // The mirror is plumbed through the audit log only — it is NOT a
    // separate Extension layer because no handler reads it directly.
    let minio_mirror: Arc<Option<MinioMirror>> = match cfg.minio.clone() {
        Some(mc) => match MinioMirror::connect(mc).await {
            Ok(m) => {
                // `connect` only wires the SDK client; surface a clean
                // PutObject path early by pinging on boot. A failing
                // ping doesn't take down the server — `/readyz` will
                // continue to report `minio: fail` until it recovers.
                if let Err(e) = m.ping().await {
                    tracing::warn!(
                        error = %e,
                        "MinIO ping failed at boot; mirror enabled but reporting unhealthy"
                    );
                } else {
                    tracing::info!("MinIO audit mirror connected");
                }
                Arc::new(Some(m))
            }
            Err(e) => {
                tracing::warn!(error = %e, "MinIO connect failed; continuing without audit mirror");
                Arc::new(None)
            }
        },
        None => {
            tracing::info!("MinIO not configured (no access key); skipping audit mirror");
            Arc::new(None)
        }
    };

    // Magic-link + recovery-code stores. Both are thin Postgres
    // wrappers; no external deps to fail at boot. Construct before
    // the audit log because `PostgresAuditLog::new` consumes `pool`.
    let magic_link_store = Arc::new(PostgresMagicLinkStore::new(pool.clone()));
    let recovery_store = Arc::new(PostgresRecoveryCodeStore::new(pool.clone()));
    let submissions_store = Arc::new(submissions::PostgresSubmissionStore::new(pool.clone()));
    let supporter_store = Arc::new(supporters::PostgresSupporterStore::new(pool.clone()));
    let orders_store = Arc::new(orders::PostgresOrderStore::new(pool.clone()));
    // Site-wide staff role store. Read by `get_me` (to surface roles
    // in MeResponse) and by the admin extractors that gate /v1/admin/*.
    // Constructed BEFORE the audit log because the audit constructor
    // moves `pool`.
    let staff_roles_store: Arc<PostgresStaffRoleStore> =
        Arc::new(PostgresStaffRoleStore::new(pool.clone()));

    // Build the audit log with the optional mirror. `with_mirror(None)`
    // is the no-mirror path; `Some(...)` wires best-effort PUTs.
    let mirror_for_audit: Option<Arc<MinioMirror>> = minio_mirror.as_ref().clone().map(Arc::new);
    let audit: Arc<dyn AuditLog> =
        Arc::new(PostgresAuditLog::new(pool.clone()).with_mirror(mirror_for_audit));

    // Type-erased handle for the StaffRoleStore extension. The admin
    // extractors look up `Arc<dyn StaffRoleStore>` from request
    // extensions; `get_me` does the same. The concrete `staff_roles_store`
    // (constructed earlier) stays alongside for the bootstrap call below
    // because the function takes `&S: StaffRoleStore` directly.
    let staff_roles_dyn: Arc<dyn StaffRoleStore> = staff_roles_store.clone();

    // Idempotently grant `admin` to every handle in
    // STARSTATS_BOOTSTRAP_ADMIN_HANDLES (comma-separated). Failures
    // inside the bootstrap (handle not found, audit-log write fail)
    // are logged and DO NOT abort startup -- a typo in the env var
    // shouldn't keep the server down.
    if let Err(e) = staff_roles::bootstrap_admins_from_env(
        users.as_ref(),
        staff_roles_store.as_ref(),
        audit.as_ref(),
        "STARSTATS_BOOTSTRAP_ADMIN_HANDLES",
    )
    .await
    {
        tracing::error!(error = ?e, "staff_roles bootstrap returned an error");
    }

    // KEK for envelope encryption at rest (TOTP secrets and SMTP
    // password). Loaded or generated; missing file is auto-fixed
    // (with 0600 perms). Moved ahead of the mailer init so the DB
    // SMTP config — which decrypts the password under this key — can
    // feed the initial mailer build.
    let kek = Arc::new(Kek::load_or_generate(&cfg.kek.path)?);
    tracing::info!(path = %cfg.kek.path.display(), "KEK loaded");

    // Trait import needed for the `.get()` / `.put()` calls on
    // `Arc<PostgresSmtpConfigStore>` immediately below.
    use crate::smtp_config_store::SmtpConfigStore as _;

    // DB-backed SMTP config store. The singleton row is seeded by
    // migration 0020 so `get()` always returns; the `enabled` flag
    // decides whether we honour it.
    let smtp_config_store = Arc::new(smtp_config_store::PostgresSmtpConfigStore::new(
        pool.clone(),
    ));

    // Mail transport precedence:
    //   1. DB row when `enabled = true` — admin-managed via /v1/admin/smtp.
    //   2. Env-based SmtpConfig (existing posture) when DB is disabled
    //      or unreadable.
    //   3. NoopMailer fallback (built into `mail::build_mailer`).
    //
    // The chosen transport is wrapped in `SwappableMailer` so the
    // admin save flow can hot-reload it without restarting the server.
    let initial_mailer: Arc<dyn Mailer> = match smtp_config_store.get(&kek).await {
        Ok(rec) if rec.enabled => {
            tracing::info!(
                host = %rec.host,
                "SMTP: using DB-managed config (admin set enabled = true)"
            );
            mail::build_mailer_from_record(&rec)
        }
        Ok(_) => {
            tracing::info!("SMTP: DB config disabled; falling back to env-based config");
            mail::build_mailer(cfg.smtp.as_ref())
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "SMTP: DB config read failed; falling back to env-based config"
            );
            mail::build_mailer(cfg.smtp.as_ref())
        }
    };
    let mailer_swap = Arc::new(mail::SwappableMailer::new(initial_mailer));
    let mailer: Arc<dyn Mailer> = mailer_swap.clone();

    // HTTP client for the RSI bio scrape. A reqwest build failure
    // here is fatal: there is no degraded mode for "we couldn't
    // configure TLS" — the verify endpoint would return 503 forever.
    let rsi_client: Arc<dyn rsi_verify::RsiClient> = Arc::new(rsi_verify::HttpRsiClient::new()?);

    // Tauri auto-updater config. The struct itself is always
    // constructed (default path); the file at that path may be
    // absent — the handler treats absence as "no update yet" and
    // returns 204 without erroring.
    let updater_cfg = Arc::new(cfg.updater.clone());
    tracing::info!(
        manifest_path = %cfg.updater.manifest_path.display(),
        "updater manifest path configured"
    );

    let server_key = ServerKey::load_or_generate(&cfg.jwt.key_path)?;
    tracing::info!(kid = %server_key.kid, "server JWT key loaded");

    let key_pem = std::fs::read_to_string(&cfg.jwt.key_path)?;
    let jwks_doc = Arc::new(well_known::JwksDocument::from_server_key(
        &server_key,
        &key_pem,
    )?);
    let discovery_cfg = Arc::new(well_known::DiscoveryConfig {
        issuer: cfg.jwt.issuer.clone(),
    });

    // The issuer mints tokens for /v1/auth/*; the verifier checks
    // incoming bearer tokens on every protected route. Both share
    // the same key.
    let issuer = Arc::new(TokenIssuer::new(
        server_key.clone(),
        cfg.jwt.issuer.clone(),
        cfg.jwt.audience.clone(),
    ));
    let verifier = Arc::new(AuthVerifier::new(
        server_key,
        cfg.jwt.issuer.clone(),
        cfg.jwt.audience.clone(),
    ));

    // Per-feature route builders live alongside their handlers
    // (`{feature}::routes()`); main just composes them. Each builder
    // wires whatever `State<_>` shape its handlers need; the shared
    // `Extension<_>`s (verifier, issuer, audit, …) are layered onto
    // the merged outer router below.
    let auth_router = auth_routes::routes(users.clone());
    let device_router = device_routes::routes(devices, users.clone());
    let sharing_router = sharing_routes::routes(users.clone(), orgs.clone(), store.clone());
    let rsi_router = rsi_verify_routes::routes(users.clone());
    let profile_router = rsi_profile_routes::routes(users.clone(), profiles.clone());
    let rsi_orgs_router = rsi_org_routes::routes(users.clone(), rsi_orgs.clone());
    let hangar_router = hangar_routes::routes(hangars);
    let preferences_router = preferences_routes::routes(preferences);
    let magic_router = magic_link_routes::routes(users.clone(), magic_link_store);
    let totp_router = totp_routes::routes(users.clone(), recovery_store);
    let org_router = org_routes::routes(orgs, users.clone());
    let reference_router = reference_routes::routes(reference_store.clone());
    let submission_router = submission_routes::routes(submissions_store.clone());
    // Admin sub-routers — gated by RequireAdmin / RequireModerator
    // extractors which read `Arc<dyn StaffRoleStore>` from request
    // extensions (layered onto the outer `app` below). admin_routes
    // exposes the extractors + a parameterless skeleton; the submission
    // moderation routes mount under it.
    let admin_router = admin_routes::router()
        .merge(admin_submission_routes::router(submissions_store))
        .merge(smtp_admin_routes::router(
            smtp_config_store.clone(),
            users.clone(),
        ));
    let supporter_router = supporter_routes::routes(supporter_store.clone());
    let donate_state =
        revolut_routes::build_state(orders_store, supporter_store, cfg.revolut.as_ref());
    let donate_router = revolut_routes::routes(donate_state);

    let app = Router::new()
        .route("/healthz", get(health::live))
        .route("/readyz", get(health::ready))
        .route("/metrics", get(health::metrics))
        .route("/.well-known/jwks.json", get(well_known::jwks))
        .route(
            "/.well-known/openid-configuration",
            get(well_known::openid_configuration),
        )
        // Cap ingest payloads at 4 MB. The Tauri client batches by
        // count, not size, but a malicious or misconfigured client
        // could otherwise POST hundreds of MB before the server rejects.
        // axum's default is 2 MB; we go slightly higher to account for
        // dense JSON-encoded event arrays from a long offline session.
        .route(
            "/v1/ingest",
            post(ingest::handle::<PostgresStore>).layer(DefaultBodyLimit::max(4 * 1024 * 1024)),
        )
        .route("/v1/me/events", get(query::list_events::<PostgresStore>))
        .route("/v1/me/summary", get(query::summary::<PostgresStore>))
        .route("/v1/me/timeline", get(query::timeline::<PostgresStore>))
        .route(
            "/v1/me/metrics/event-types",
            get(query::metrics_event_types::<PostgresStore>),
        )
        .route(
            "/v1/me/metrics/sessions",
            get(query::metrics_sessions::<PostgresStore>),
        )
        .route(
            "/v1/me/ingest-history",
            get(query::ingest_history::<PostgresStore>),
        )
        .route(
            "/v1/me/location/current",
            get(query::location_current::<PostgresStore>),
        )
        .route(
            "/v1/me/location/trace",
            get(query::location_trace::<PostgresStore>),
        )
        .route(
            "/v1/me/location/breakdown",
            get(query::location_breakdown::<PostgresStore>),
        )
        .route(
            "/v1/me/stats/combat",
            get(query::stats_combat::<PostgresStore>),
        )
        .route(
            "/v1/me/stats/travel",
            get(query::stats_travel::<PostgresStore>),
        )
        .route(
            "/v1/me/stats/loadout",
            get(query::stats_loadout::<PostgresStore>),
        )
        .route(
            "/v1/me/stats/stability",
            get(query::stats_stability::<PostgresStore>),
        )
        .route(
            "/v1/me/commerce/recent",
            get(query::commerce_recent::<PostgresStore>),
        )
        .route(
            "/v1/updater/:target/:arch/:current_version",
            get(update_routes::check_for_update),
        )
        .with_state(store)
        .merge(auth_router)
        .merge(device_router)
        .merge(sharing_router)
        .merge(rsi_router)
        .merge(profile_router)
        .merge(rsi_orgs_router)
        .merge(hangar_router)
        .merge(preferences_router)
        .merge(magic_router)
        .merge(totp_router)
        .merge(org_router)
        .merge(reference_router)
        .merge(parser_def_routes::routes())
        .merge(submission_router)
        .merge(admin_router)
        .merge(supporter_router)
        .merge(donate_router)
        // OpenAPI spec at /openapi.json — purely additive, no auth.
        .merge(openapi::router())
        .layer(Extension(verifier))
        .layer(Extension(issuer))
        .layer(Extension(audit))
        .layer(Extension(device_store_dyn))
        .layer(Extension(staff_roles_dyn))
        .layer(Extension(jwks_doc))
        .layer(Extension(discovery_cfg))
        .layer(Extension(prometheus))
        .layer(Extension(health_pool))
        .layer(Extension(spicedb))
        .layer(Extension(minio_mirror))
        .layer(Extension(mailer))
        .layer(Extension(mailer_swap))
        .layer(Extension(rsi_client))
        .layer(Extension(kek))
        .layer(Extension(updater_cfg));

    // Daily refresh of community-API-sourced reference data across
    // all four categories (vehicle / weapon / item / location).
    // Runs once at startup and every 24h thereafter (1h on failure so
    // a transient wiki outage at boot doesn't leave the cache empty
    // for a full day). Best-effort: failures log per-category and we
    // keep serving whatever is already cached — stale data is more
    // useful than no data. `sleep` (not `interval`) is deliberate: we
    // don't want catch-up firing if the tokio task ever stalls on an
    // upstream. The sleep cadence drops to REFRESH_FAIL if ANY
    // category failed; partial success still gets a retry quickly so
    // we close the gap on the missing categories.
    const REFRESH_OK: Duration = Duration::from_secs(24 * 3600);
    const REFRESH_FAIL: Duration = Duration::from_secs(3600);
    const CATEGORIES: [ReferenceCategory; 4] = [
        ReferenceCategory::Vehicle,
        ReferenceCategory::Weapon,
        ReferenceCategory::Item,
        ReferenceCategory::Location,
    ];
    {
        let reference_store = reference_store.clone();
        let reference_client = reference_client.clone();
        tokio::spawn(async move {
            loop {
                let mut any_failed = false;
                for cat in CATEGORIES {
                    match reference_client.fetch_category(cat).await {
                        ReferenceFetchOutcomeCategory::Entries(entries) => {
                            match reference_store.upsert_entries(&entries).await {
                                Ok(n) => {
                                    tracing::info!(
                                        category = cat.as_str(),
                                        rows = n,
                                        "reference data refreshed"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        error = %e,
                                        category = cat.as_str(),
                                        "reference upsert failed"
                                    );
                                    any_failed = true;
                                }
                            }
                        }
                        ReferenceFetchOutcomeCategory::UpstreamUnavailable => {
                            tracing::warn!(
                                category = cat.as_str(),
                                "reference upstream unavailable; retaining cached data"
                            );
                            any_failed = true;
                        }
                    }
                }
                let next = if any_failed { REFRESH_FAIL } else { REFRESH_OK };
                tokio::time::sleep(next).await;
            }
        });
    }

    tracing::info!(bind = %cfg.bind, "starstats-server listening");
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    // `into_make_service_with_connect_info` exposes the peer SocketAddr
    // to extractors. `tower_governor::SmartIpKeyExtractor` consults
    // `X-Forwarded-For`/`Forwarded`/`X-Real-IP` first (Traefik fills
    // these in prod) and falls back to the peer addr for direct hits.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    // Drop the guard explicitly so the OTLP exporter can flush queued
    // spans before the process exits. No-op if OTEL was not configured.
    drop(otel_guard);
    Ok(())
}
