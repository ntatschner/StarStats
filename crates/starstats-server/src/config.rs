//! Server runtime configuration. Read once at startup from
//! environment, fail fast on missing required values.
//!
//! Auth model: StarStats is its own identity provider. The signing
//! keypair lives at `STARSTATS_JWT_KEY_FILE`; if absent it's generated
//! on first boot. `issuer` and `audience` are baked into both the
//! tokens we mint and the validation rules we apply.

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Resolved server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub jwt: JwtConfig,
    /// SpiceDB connection settings. May be `None` in local dev where
    /// the sidecar is not running — the server will boot in degraded
    /// mode and `/readyz` will report `spicedb: skipped`.
    pub spicedb: Option<SpicedbConfig>,
    /// MinIO/S3 audit-log mirror settings. May be `None` in local dev
    /// where the sidecar is not running — the server will boot without
    /// the mirror and `/readyz` will report `minio: skipped`. Postgres
    /// remains the source of truth for audit entries either way.
    pub minio: Option<MinioConfig>,
    /// SMTP transport for transactional email (verification links).
    /// May be `None` — when missing, the server uses a no-op mailer
    /// that logs sends instead of delivering. Signup still succeeds
    /// without SMTP; the user just never gets a verification email.
    pub smtp: Option<SmtpConfig>,
    /// Tauri auto-updater manifest config. Always present (the path
    /// has a default), but the file at that path may be absent — in
    /// which case `/v1/updater/...` returns 204 ("no update yet")
    /// rather than failing the boot.
    pub updater: UpdaterConfig,
    /// Key Encryption Key for TOTP shared secrets. Always present
    /// (default path), file auto-generated on first boot.
    pub kek: KekConfig,
    /// Revolut Business merchant API settings. Driven entirely by env
    /// vars — `None` here means at least one required knob is missing,
    /// in which case the donate routes return 503 `not_configured`
    /// rather than touching the wire. The boot path doesn't fail on
    /// missing Revolut config; donations are an optional revenue
    /// stream, not a core capability.
    pub revolut: Option<RevolutConfig>,
}

/// Revolut Business Merchant API client configuration.
///
/// Required env vars:
///   - `REVOLUT_API_KEY`        Bearer key from the merchant dashboard.
///   - `REVOLUT_WEBHOOK_SECRET` HMAC signing secret for webhook events.
///
/// Optional env vars (sensible defaults shipped):
///   - `REVOLUT_API_BASE`       Defaults to the sandbox host. Switch to
///                              `https://merchant.revolut.com` for prod.
///   - `REVOLUT_API_VERSION`    Defaults to `2024-09-01` — pinned so a
///                              future Revolut breaking change can't
///                              silently land in production.
///   - `REVOLUT_RETURN_URL`     Where Revolut's hosted checkout page
///                              redirects after a successful payment.
#[derive(Debug, Clone)]
pub struct RevolutConfig {
    pub api_key: String,
    pub webhook_secret: String,
    pub api_base: String,
    pub api_version: String,
    pub return_url: Option<String>,
}

impl RevolutConfig {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("REVOLUT_API_KEY").ok()?;
        let webhook_secret = std::env::var("REVOLUT_WEBHOOK_SECRET").ok()?;
        if api_key.is_empty() || webhook_secret.is_empty() {
            return None;
        }
        let api_base = std::env::var("REVOLUT_API_BASE")
            .unwrap_or_else(|_| "https://sandbox-merchant.revolut.com".to_string())
            .trim_end_matches('/')
            .to_string();
        let api_version =
            std::env::var("REVOLUT_API_VERSION").unwrap_or_else(|_| "2024-09-01".to_string());
        let return_url = std::env::var("REVOLUT_RETURN_URL")
            .ok()
            .filter(|s| !s.is_empty());
        Some(Self {
            api_key,
            webhook_secret,
            api_base,
            api_version,
            return_url,
        })
    }
}

#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// Path to the RSA private key PEM. Auto-generated on first boot
    /// if the file is missing.
    pub key_path: PathBuf,
    /// Value baked into every minted token's `iss` claim and required
    /// on every inbound token. Should be the public origin of this
    /// server (e.g. `https://api.example.com`).
    pub issuer: String,
    /// Value baked into every minted token's `aud` claim. Lets a
    /// third-party verifier (or future second-server topology)
    /// reject tokens that weren't minted for this audience.
    pub audience: String,
}

/// AES-256-GCM Key Encryption Key for TOTP shared secrets at rest.
///
/// The KEK is held in a server-local file (analogous to the JWT
/// signing key) and used to wrap each user's TOTP secret before it
/// touches the database. Auto-generated on first boot if the file
/// is missing — the file is the source of truth, so backing it up
/// alongside the JWT key is enough to migrate.
///
/// Rotating the KEK is a manual operation today: replace the file,
/// run a one-shot job that re-encrypts every `users.totp_secret_*`
/// row under the new key. There's no per-row key version yet; the
/// TODO is in this module.
#[derive(Debug, Clone)]
pub struct KekConfig {
    pub path: PathBuf,
}

/// SpiceDB sidecar connection settings.
///
/// Mirrors the secret-from-file pattern used by [`build_url_from_parts`]
/// for Postgres: the preshared key may be supplied directly via
/// `SPICEDB_PRESHARED_KEY` for local dev, or read from a Docker
/// secret file via `SPICEDB_PRESHARED_KEY_FILE` in production.
#[derive(Debug, Clone)]
pub struct SpicedbConfig {
    /// gRPC endpoint of the SpiceDB server, e.g.
    /// `http://spicedb:50051`. Plain HTTP is fine on the homelab
    /// container network; TLS is terminated at the ingress layer.
    pub endpoint: String,
    /// Preshared bearer token. SpiceDB uses a single static token for
    /// authn, scoped per-deployment.
    pub preshared_key: String,
}

impl SpicedbConfig {
    /// Read SpiceDB config from env. Returns `Ok(None)` when no
    /// preshared key is provided — that's the signal to boot in
    /// degraded mode (same posture as the OTLP exporter).
    pub fn from_env() -> Result<Option<Self>> {
        let endpoint = std::env::var("SPICEDB_ENDPOINT")
            .unwrap_or_else(|_| "http://spicedb:50051".to_string());

        let preshared_key = match read_preshared_key()? {
            Some(k) => k,
            None => return Ok(None),
        };

        Ok(Some(Self {
            endpoint,
            preshared_key,
        }))
    }
}

/// Read a secret from `env` (inline value) or from the file path at
/// `file_env`. Returns `Ok(None)` when neither is set. Trims trailing
/// CRLF from file contents so Docker-secrets-style mounts work without
/// the caller needing to strip newlines.
fn read_env_or_file(env: &str, file_env: &str) -> Result<Option<String>> {
    if let Ok(k) = std::env::var(env) {
        if !k.is_empty() {
            return Ok(Some(k));
        }
    }
    if let Ok(path) = std::env::var(file_env) {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read secret file at {path} (from {file_env})"))?;
        return Ok(Some(raw.trim_end_matches(['\r', '\n']).to_string()));
    }
    Ok(None)
}

/// Resolve the SpiceDB preshared key from env or secret file.
/// Returns `Ok(None)` when neither is set — caller treats that as
/// "SpiceDB not configured".
fn read_preshared_key() -> Result<Option<String>> {
    read_env_or_file("SPICEDB_PRESHARED_KEY", "SPICEDB_PRESHARED_KEY_FILE")
}

/// MinIO / S3-compatible object storage settings for the audit-log
/// mirror.
///
/// Mirrors [`SpicedbConfig`]: the access key may be supplied directly
/// via `MINIO_ACCESS_KEY` / `MINIO_SECRET_KEY` for local dev, or read
/// from a Docker secret file via `MINIO_SECRET_KEY_FILE` in
/// production. Both must resolve for [`MinioConfig::from_env`] to
/// return `Some`; otherwise the server boots without the mirror.
#[derive(Debug, Clone)]
pub struct MinioConfig {
    /// S3 endpoint URL, e.g. `http://starstats-minio:9000`. The
    /// homelab speaks plain HTTP on the container network and
    /// terminates TLS at Traefik for any externally-routed clients.
    pub endpoint: String,
    /// AWS region string. MinIO ignores it but the SDK requires one;
    /// any value matching the bucket's configured region is fine
    /// (`us-east-1` is the SDK-wide default).
    pub region: String,
    /// S3 access key (analogous to AWS access key id).
    pub access_key: String,
    /// S3 secret key. Resolved from `MINIO_SECRET_KEY` or
    /// `MINIO_SECRET_KEY_FILE` — the same secret-from-file pattern as
    /// the Postgres password and SpiceDB preshared key.
    pub secret_key: String,
    /// Bucket that receives mirrored audit entries. Should be an
    /// Object-Lock-in-Compliance-mode bucket so retained audit rows
    /// can't be tampered with after write.
    pub audit_bucket: String,
}

impl MinioConfig {
    /// Read MinIO config from env. Returns `Ok(None)` when either the
    /// access key or secret key is missing — that's the signal to
    /// boot without the audit mirror (same posture as SpiceDB).
    pub fn from_env() -> Result<Option<Self>> {
        let endpoint = std::env::var("MINIO_ENDPOINT")
            .unwrap_or_else(|_| "http://starstats-minio:9000".to_string());
        let region = std::env::var("MINIO_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let audit_bucket =
            std::env::var("MINIO_AUDIT_BUCKET").unwrap_or_else(|_| "starstats-audit".to_string());

        let access_key = match std::env::var("MINIO_ACCESS_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => return Ok(None),
        };

        let secret_key = match read_minio_secret_key()? {
            Some(k) => k,
            None => return Ok(None),
        };

        Ok(Some(Self {
            endpoint,
            region,
            access_key,
            secret_key,
            audit_bucket,
        }))
    }
}

/// Resolve the MinIO secret key from env or secret file. Returns
/// `Ok(None)` when neither is set.
fn read_minio_secret_key() -> Result<Option<String>> {
    read_env_or_file("MINIO_SECRET_KEY", "MINIO_SECRET_KEY_FILE")
}

/// SMTP transport settings for transactional email.
///
/// Same posture as [`SpicedbConfig`] / [`MinioConfig`]: returning
/// `Ok(None)` means "no SMTP wired" and the caller falls back to a
/// no-op mailer. Production reads `SMTP_URL` (which may include
/// credentials) directly, or splits the password into a Docker
/// secret file via `SMTP_PASSWORD_FILE` (a future enhancement —
/// today the inline URL is sufficient for the homelab).
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    /// Connection URL. Examples:
    /// `smtps://user:pass@smtp.example.com:465` (implicit TLS) or
    /// `smtp://user:pass@smtp.example.com:587` (STARTTLS). Lettre
    /// parses the scheme and selects the right transport mode.
    pub url: String,
    /// `From:` address on outbound mail. Defaults to
    /// `noreply@starstats.local` so local-dev sends don't accidentally
    /// borrow a real domain's reputation.
    pub from_addr: String,
    /// Display name on the `From:` header. Defaults to `StarStats`.
    pub from_name: String,
    /// Origin to embed in verification links — should be the public
    /// origin of the web app (e.g. `https://app.example.com`). Falls
    /// back to `STARSTATS_JWT_ISSUER` since that's already the canonical
    /// public URL of this deployment.
    pub web_origin: String,
}

impl SmtpConfig {
    /// Read SMTP config from env. Returns `Ok(None)` when `SMTP_URL`
    /// isn't set — that's the signal to wire a no-op mailer (same
    /// posture as SpiceDB / MinIO). When set, all other knobs fall
    /// back to sensible defaults so a single env var is enough to
    /// turn email on.
    pub fn from_env() -> Result<Option<Self>> {
        let url = match std::env::var("SMTP_URL") {
            Ok(u) if !u.is_empty() => u,
            _ => return Ok(None),
        };

        let from_addr = std::env::var("SMTP_FROM_ADDR")
            .unwrap_or_else(|_| "noreply@starstats.local".to_string());
        let from_name = std::env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "StarStats".to_string());
        // Prefer the explicit knob; fall back to JWT issuer because
        // that's the same origin every other public URL in this
        // deployment derives from.
        let web_origin = std::env::var("SMTP_WEB_ORIGIN")
            .or_else(|_| std::env::var("STARSTATS_JWT_ISSUER"))
            .context("SMTP_WEB_ORIGIN or STARSTATS_JWT_ISSUER required when SMTP_URL is set")?;

        Ok(Some(Self {
            url,
            from_addr,
            from_name,
            web_origin,
        }))
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind = std::env::var("STARSTATS_BIND")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
            .parse()
            .context("STARSTATS_BIND is not a valid socket address")?;

        // Compose deployment uses Postgres password files mounted at
        // /run/secrets/starstats_db_password. Direct DATABASE_URL is
        // accepted for local dev.
        let database_url = if let Ok(url) = std::env::var("DATABASE_URL") {
            url
        } else {
            build_url_from_parts()?
        };

        let jwt = JwtConfig::from_env()?;
        let spicedb = SpicedbConfig::from_env()?;
        let minio = MinioConfig::from_env()?;
        let smtp = SmtpConfig::from_env()?;
        let updater = UpdaterConfig::from_env();
        let kek = KekConfig::from_env();
        let revolut = RevolutConfig::from_env();
        if revolut.is_some() {
            tracing::info!("Revolut Business merchant API configured");
        } else {
            tracing::info!(
                "Revolut Business merchant API not configured (donate endpoints will return 503)"
            );
        }

        Ok(Self {
            bind,
            database_url,
            jwt,
            spicedb,
            minio,
            smtp,
            updater,
            kek,
            revolut,
        })
    }
}

impl KekConfig {
    pub fn from_env() -> Self {
        let path = std::env::var("STARSTATS_KEK_FILE")
            .unwrap_or_else(|_| "/var/lib/starstats/totp-kek.bin".to_string())
            .into();
        Self { path }
    }
}

/// Tauri auto-updater configuration.
///
/// Unlike the optional integrations above, this struct is always
/// constructed — every deployment has a sensible default path. The
/// runtime check for "is there actually an update" happens per-request
/// when the handler tries to open the file: a missing file is the
/// "no update yet" signal, NOT a misconfiguration.
#[derive(Debug, Clone)]
pub struct UpdaterConfig {
    /// Filesystem path to the Tauri 2 unified manifest JSON. Release
    /// tooling writes this file when a new build is available; the
    /// server reads it per request.
    pub manifest_path: PathBuf,
}

impl UpdaterConfig {
    /// Read the updater config from env. Returns a fully-populated
    /// struct with a sensible default path so the boot flow never
    /// fails on a missing knob — the path's *contents* are checked
    /// at request time.
    pub fn from_env() -> Self {
        let manifest_path = std::env::var("STARSTATS_UPDATER_MANIFEST_PATH")
            .unwrap_or_else(|_| "/var/lib/starstats/updater-manifest.json".to_string())
            .into();
        Self { manifest_path }
    }
}

impl JwtConfig {
    pub fn from_env() -> Result<Self> {
        let key_path = std::env::var("STARSTATS_JWT_KEY_FILE")
            .unwrap_or_else(|_| "/var/lib/starstats/jwt-key.pem".into())
            .into();
        let issuer =
            std::env::var("STARSTATS_JWT_ISSUER").context("STARSTATS_JWT_ISSUER not set")?;
        let audience =
            std::env::var("STARSTATS_JWT_AUDIENCE").unwrap_or_else(|_| "starstats".to_string());
        Ok(Self {
            key_path,
            issuer,
            audience,
        })
    }
}

fn build_url_from_parts() -> Result<String> {
    let host = std::env::var("STARSTATS_DB_HOST").unwrap_or_else(|_| "postgres".into());
    let port = std::env::var("STARSTATS_DB_PORT").unwrap_or_else(|_| "5432".into());
    let user = std::env::var("STARSTATS_DB_USER").unwrap_or_else(|_| "starstats_app".into());
    let db = std::env::var("STARSTATS_DB_NAME").unwrap_or_else(|_| "starstats".into());

    let password = read_password()?;

    // sslmode default is `prefer`: encrypts when the server supports
    // it, falls back to plaintext otherwise. Operators can pin it to
    // `require`/`verify-ca`/`verify-full` via STARSTATS_DB_SSLMODE
    // when their PG instance has TLS configured. We don't default to
    // `require` because most homelab Postgres containers ship without
    // a server cert and would refuse to connect — that's a foot-gun
    // for first-run boot. `prefer` lets the deployment opt up.
    let sslmode = std::env::var("STARSTATS_DB_SSLMODE").unwrap_or_else(|_| "prefer".into());

    Ok(format!(
        "postgres://{user}:{password}@{host}:{port}/{db}?sslmode={sslmode}"
    ))
}

fn read_password() -> Result<String> {
    if let Ok(p) = std::env::var("STARSTATS_DB_PASSWORD") {
        return Ok(p);
    }
    let path = std::env::var("STARSTATS_DB_PASSWORD_FILE")
        .unwrap_or_else(|_| "/run/secrets/starstats_db_password".into());
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("read password file at {path}"))?;
    Ok(raw.trim_end_matches(['\r', '\n']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global. Serialize tests that touch them so
    // a parallel run doesn't see another test's mutations.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_spicedb_env() {
        std::env::remove_var("SPICEDB_ENDPOINT");
        std::env::remove_var("SPICEDB_PRESHARED_KEY");
        std::env::remove_var("SPICEDB_PRESHARED_KEY_FILE");
    }

    #[test]
    fn spicedb_config_returns_none_when_no_key_set() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_spicedb_env();
        let cfg = SpicedbConfig::from_env().unwrap();
        assert!(cfg.is_none(), "missing key should map to None (degraded)");
    }

    #[test]
    fn spicedb_config_reads_inline_key_with_default_endpoint() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_spicedb_env();
        std::env::set_var("SPICEDB_PRESHARED_KEY", "secret-token");

        let cfg = SpicedbConfig::from_env().unwrap().expect("config present");
        assert_eq!(cfg.preshared_key, "secret-token");
        assert_eq!(cfg.endpoint, "http://spicedb:50051");

        clear_spicedb_env();
    }

    #[test]
    fn spicedb_config_honours_explicit_endpoint() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_spicedb_env();
        std::env::set_var("SPICEDB_PRESHARED_KEY", "k");
        std::env::set_var("SPICEDB_ENDPOINT", "http://localhost:50052");

        let cfg = SpicedbConfig::from_env().unwrap().expect("config present");
        assert_eq!(cfg.endpoint, "http://localhost:50052");

        clear_spicedb_env();
    }

    fn clear_minio_env() {
        std::env::remove_var("MINIO_ENDPOINT");
        std::env::remove_var("MINIO_REGION");
        std::env::remove_var("MINIO_ACCESS_KEY");
        std::env::remove_var("MINIO_SECRET_KEY");
        std::env::remove_var("MINIO_SECRET_KEY_FILE");
        std::env::remove_var("MINIO_AUDIT_BUCKET");
    }

    #[test]
    fn minio_config_returns_none_when_no_credentials_set() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_minio_env();
        let cfg = MinioConfig::from_env().unwrap();
        assert!(
            cfg.is_none(),
            "missing credentials should map to None (skipped)"
        );
    }

    #[test]
    fn minio_config_returns_none_when_only_access_key_set() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_minio_env();
        std::env::set_var("MINIO_ACCESS_KEY", "key");
        // No secret -> still None.
        let cfg = MinioConfig::from_env().unwrap();
        assert!(cfg.is_none());
        clear_minio_env();
    }

    #[test]
    fn minio_config_reads_inline_credentials_with_defaults() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_minio_env();
        std::env::set_var("MINIO_ACCESS_KEY", "starstats");
        std::env::set_var("MINIO_SECRET_KEY", "supersecret");

        let cfg = MinioConfig::from_env().unwrap().expect("config present");
        assert_eq!(cfg.access_key, "starstats");
        assert_eq!(cfg.secret_key, "supersecret");
        assert_eq!(cfg.endpoint, "http://starstats-minio:9000");
        assert_eq!(cfg.region, "us-east-1");
        assert_eq!(cfg.audit_bucket, "starstats-audit");

        clear_minio_env();
    }

    fn clear_smtp_env() {
        std::env::remove_var("SMTP_URL");
        std::env::remove_var("SMTP_FROM_ADDR");
        std::env::remove_var("SMTP_FROM_NAME");
        std::env::remove_var("SMTP_WEB_ORIGIN");
        std::env::remove_var("STARSTATS_JWT_ISSUER");
    }

    #[test]
    fn smtp_config_returns_none_when_url_missing() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_smtp_env();
        let cfg = SmtpConfig::from_env().unwrap();
        assert!(cfg.is_none(), "missing SMTP_URL should map to None");
    }

    #[test]
    fn smtp_config_falls_back_to_jwt_issuer_for_origin() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_smtp_env();
        std::env::set_var("SMTP_URL", "smtps://u:p@smtp.example.com:465");
        std::env::set_var("STARSTATS_JWT_ISSUER", "https://app.example.com");

        let cfg = SmtpConfig::from_env().unwrap().expect("config present");
        assert_eq!(cfg.web_origin, "https://app.example.com");
        assert_eq!(cfg.from_addr, "noreply@starstats.local");
        assert_eq!(cfg.from_name, "StarStats");

        clear_smtp_env();
    }

    #[test]
    fn smtp_config_honours_explicit_overrides() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_smtp_env();
        std::env::set_var("SMTP_URL", "smtp://u:p@smtp.example.com:587");
        std::env::set_var("SMTP_FROM_ADDR", "hello@app.example.com");
        std::env::set_var("SMTP_FROM_NAME", "Stats");
        std::env::set_var("SMTP_WEB_ORIGIN", "https://app.example.com");

        let cfg = SmtpConfig::from_env().unwrap().expect("config present");
        assert_eq!(cfg.from_addr, "hello@app.example.com");
        assert_eq!(cfg.from_name, "Stats");
        assert_eq!(cfg.web_origin, "https://app.example.com");

        clear_smtp_env();
    }

    #[test]
    fn minio_config_honours_explicit_overrides() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_minio_env();
        std::env::set_var("MINIO_ACCESS_KEY", "k");
        std::env::set_var("MINIO_SECRET_KEY", "s");
        std::env::set_var("MINIO_ENDPOINT", "http://localhost:9000");
        std::env::set_var("MINIO_REGION", "eu-west-2");
        std::env::set_var("MINIO_AUDIT_BUCKET", "custom-audit");

        let cfg = MinioConfig::from_env().unwrap().expect("config present");
        assert_eq!(cfg.endpoint, "http://localhost:9000");
        assert_eq!(cfg.region, "eu-west-2");
        assert_eq!(cfg.audit_bucket, "custom-audit");

        clear_minio_env();
    }
}
