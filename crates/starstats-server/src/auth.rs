//! Self-hosted JWT authentication.
//!
//! StarStats is its own identity provider. The server holds an RS256
//! keypair on disk; [`TokenIssuer`] signs access tokens at login time,
//! [`AuthVerifier`] checks signatures + standard claims on incoming
//! requests, [`AuthenticatedUser`] is the extracted identity injected
//! into handlers.
//!
//! Why self-hosted instead of brokering through Authentik / GitHub
//! OIDC: the user model is anchored to RSI handles which no external
//! IdP knows about, and we want first-class device-pairing and
//! revocation. The JWKS endpoint (Phase 5) re-publishes the public
//! key for any third-party tool that needs to verify our tokens.
//!
//! HS256 is rejected — see the [`AuthError::UnsupportedAlgorithm`]
//! arm. Symmetric algorithms invalidate the asymmetric trust model.

use async_trait::async_trait;
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json, RequestPartsExt,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use jsonwebtoken::{
    decode, decode_header, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing or malformed bearer token")]
    MissingToken,
    #[error("token header could not be decoded: {0}")]
    BadHeader(String),
    #[error("token uses unsupported algorithm")]
    UnsupportedAlgorithm,
    #[error("token references unknown signing key")]
    UnknownKey,
    #[error("token rejected: {0}")]
    InvalidToken(String),
    #[error("keypair load/generate failed: {0}")]
    Keypair(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": "unauthorized",
            "detail": self.to_string(),
        });
        (StatusCode::UNAUTHORIZED, Json(body)).into_response()
    }
}

/// Standard JWT claims plus the StarStats-specific identity fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub aud: AudClaim,
    pub exp: usize,
    #[serde(default)]
    pub iat: usize,
    /// Free-form display name. For user tokens this is the email or
    /// claimed RSI handle; for device tokens it's the device label
    /// from the pairing flow.
    #[serde(default)]
    pub preferred_username: String,
    /// Token kind. Lets handlers refuse e.g. a device token where a
    /// human session is required (token introspection endpoints).
    #[serde(default)]
    pub token_type: TokenType,
    /// Set on device tokens; identifies the row in `devices`. The
    /// extractor consults this field on every request to enforce
    /// revocation — a deleted/revoked device row makes its JWT
    /// invalid even though it hasn't expired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    #[default]
    User,
    Device,
    /// Short-lived (5 min) token issued after a successful
    /// password/magic-link redemption when the account has TOTP
    /// enabled. Only `/v1/auth/totp/verify-login` accepts it; every
    /// other protected route's `require_user_token` check rejects
    /// it because it doesn't match `TokenType::User`. The handler
    /// trades it for a real user JWT after the second factor is
    /// verified.
    LoginInterim,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AudClaim {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub sub: String,
    pub preferred_username: String,
    pub token_type: TokenType,
    /// Populated when the bearer token was a device JWT. Routes that
    /// only make sense for paired devices (or that want to log per
    /// device) read this; user-token routes ignore it.
    pub device_id: Option<Uuid>,
}

impl From<Claims> for AuthenticatedUser {
    fn from(c: Claims) -> Self {
        Self {
            sub: c.sub,
            preferred_username: c.preferred_username,
            token_type: c.token_type,
            device_id: c.device_id,
        }
    }
}

// -- Keypair ---------------------------------------------------------

/// Holds both halves of the server's signing keypair. Created once at
/// startup; cloned (Arc) into [`TokenIssuer`] and [`AuthVerifier`].
#[derive(Clone)]
pub struct ServerKey {
    pub kid: String,
    pub encoding: Arc<EncodingKey>,
    pub decoding: Arc<DecodingKey>,
}

impl ServerKey {
    /// Load PEM from `path`, generating a fresh 2048-bit RSA keypair
    /// (and writing it to `path` with 0600) if the file is absent.
    /// `kid` is derived from the file's mtime so verifiers can pin a
    /// specific key version when we add rotation later.
    pub fn load_or_generate(path: &std::path::Path) -> Result<Self, AuthError> {
        let pem = if path.exists() {
            std::fs::read_to_string(path)
                .map_err(|e| AuthError::Keypair(format!("read {path:?}: {e}")))?
        } else {
            generate_and_persist(path)?
        };

        let encoding = EncodingKey::from_rsa_pem(pem.as_bytes())
            .map_err(|e| AuthError::Keypair(format!("encoding key: {e}")))?;

        // The decoding key needs the public half. jsonwebtoken can
        // derive it from the same PEM (it accepts both RSA private
        // and public PEMs and pulls out what it needs).
        let decoding = decoding_from_private_pem(&pem)?;

        let kid = derive_kid(path)?;

        Ok(Self {
            kid,
            encoding: Arc::new(encoding),
            decoding: Arc::new(decoding),
        })
    }
}

fn generate_and_persist(path: &std::path::Path) -> Result<String, AuthError> {
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::RsaPrivateKey;

    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048)
        .map_err(|e| AuthError::Keypair(format!("rsa keygen: {e}")))?;
    let pem = priv_key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .map_err(|e| AuthError::Keypair(format!("pkcs1 pem: {e}")))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AuthError::Keypair(format!("create dir {parent:?}: {e}")))?;
    }
    std::fs::write(path, pem.as_bytes())
        .map_err(|e| AuthError::Keypair(format!("write {path:?}: {e}")))?;
    set_secret_perms(path);

    tracing::info!(path = %path.display(), "generated new server JWT keypair");
    Ok(pem.to_string())
}

#[cfg(unix)]
fn set_secret_perms(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perm = meta.permissions();
        perm.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perm);
    }
}

#[cfg(not(unix))]
fn set_secret_perms(_path: &std::path::Path) {
    // Windows ACLs are out of scope; the file lives in $DOCKERDIR and
    // is normally only readable by the container user.
}

fn decoding_from_private_pem(pem: &str) -> Result<DecodingKey, AuthError> {
    use rsa::pkcs1::{EncodeRsaPublicKey, LineEnding};
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::{RsaPrivateKey, RsaPublicKey};

    // Try PKCS#8 first, fall back to PKCS#1 (jsonwebtoken's own
    // generator emits PKCS#1).
    let priv_key = RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| {
            use rsa::pkcs1::DecodeRsaPrivateKey;
            RsaPrivateKey::from_pkcs1_pem(pem)
        })
        .map_err(|e| AuthError::Keypair(format!("parse private key: {e}")))?;

    let pub_key = RsaPublicKey::from(&priv_key);
    let pub_pem = pub_key
        .to_pkcs1_pem(LineEnding::LF)
        .map_err(|e| AuthError::Keypair(format!("public pem: {e}")))?;

    DecodingKey::from_rsa_pem(pub_pem.as_bytes())
        .map_err(|e| AuthError::Keypair(format!("decoding key: {e}")))
}

fn derive_kid(path: &std::path::Path) -> Result<String, AuthError> {
    use sha2::{Digest, Sha256};
    // Read the actual key file. If this fails the previous behaviour
    // silently used SHA256("") as the kid, which means every token
    // minted under the broken read carried a bogus kid that no JWKS
    // entry would match — clients would all fail to verify and the
    // operator would have no log line indicating the read failed.
    // Surface the error instead so boot fails loudly.
    let bytes = std::fs::read(path)
        .map_err(|e| AuthError::Keypair(format!("derive_kid: read {}: {e}", path.display())))?;
    let digest = Sha256::digest(&bytes);
    // 12 hex chars is plenty for distinguishing keys; the full hash
    // never leaves the server.
    Ok(hex::encode(&digest[..6]))
}

// -- Token issuer ----------------------------------------------------

/// Mints user and device tokens. Constructed at boot in main() and
/// wired into the router as an Extension for the auth handlers.
#[derive(Clone)]
pub struct TokenIssuer {
    key: ServerKey,
    issuer: String,
    audience: String,
    user_ttl_secs: u64,
    /// Used by [`TokenIssuer::sign_device`] which Slice 3 wires up
    /// when the device-pairing handlers land.
    #[allow(dead_code)]
    device_ttl_secs: u64,
}

impl TokenIssuer {
    pub fn new(key: ServerKey, issuer: String, audience: String) -> Self {
        Self {
            key,
            issuer,
            audience,
            user_ttl_secs: 3600,         // 1 h for web sessions
            device_ttl_secs: 90 * 86400, // 90 d for device tokens
        }
    }

    pub fn sign_user(&self, sub: &str, preferred_username: &str) -> Result<String, AuthError> {
        self.sign(
            sub,
            preferred_username,
            TokenType::User,
            None,
            self.user_ttl_secs,
        )
    }

    /// Mint a 5-minute interim token used between the password/magic-
    /// link leg of login and the TOTP verification leg. The
    /// audience + issuer are unchanged so the verifier accepts it,
    /// but the `token_type` claim makes every regular protected
    /// endpoint's `require_user_token` reject it.
    pub fn sign_login_interim(
        &self,
        sub: &str,
        preferred_username: &str,
    ) -> Result<String, AuthError> {
        self.sign(
            sub,
            preferred_username,
            TokenType::LoginInterim,
            None,
            300, // 5 minutes — long enough to type a 6-digit code
        )
    }

    /// Mint a device JWT. `device_id` ties the token to the row in
    /// `devices` — the extractor uses it to enforce revocation on
    /// every request. The desktop client never sees the device row's
    /// uuid; it stays opaque inside the token.
    pub fn sign_device(
        &self,
        sub: &str,
        label: &str,
        device_id: Uuid,
    ) -> Result<String, AuthError> {
        self.sign(
            sub,
            label,
            TokenType::Device,
            Some(device_id),
            self.device_ttl_secs,
        )
    }

    fn sign(
        &self,
        sub: &str,
        preferred_username: &str,
        token_type: TokenType,
        device_id: Option<Uuid>,
        ttl_secs: u64,
    ) -> Result<String, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::Keypair(e.to_string()))?
            .as_secs() as usize;
        let claims = Claims {
            sub: sub.to_owned(),
            iss: self.issuer.clone(),
            aud: AudClaim::One(self.audience.clone()),
            exp: now + ttl_secs as usize,
            iat: now,
            preferred_username: preferred_username.to_owned(),
            token_type,
            device_id,
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.key.kid.clone());
        jsonwebtoken::encode(&header, &claims, &self.key.encoding)
            .map_err(|e| AuthError::InvalidToken(e.to_string()))
    }
}

// -- Verifier --------------------------------------------------------

#[derive(Clone)]
pub struct AuthVerifier {
    key: ServerKey,
    expected_issuer: String,
    expected_audience: String,
    leeway_seconds: u64,
}

impl AuthVerifier {
    pub fn new(key: ServerKey, expected_issuer: String, expected_audience: String) -> Self {
        Self {
            key,
            expected_issuer,
            expected_audience,
            leeway_seconds: 60,
        }
    }

    pub fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let header = decode_header(token).map_err(|e| AuthError::BadHeader(e.to_string()))?;

        // RSA / ECDSA only — see module-level note on HS*.
        match header.alg {
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {}
            Algorithm::ES256 | Algorithm::ES384 => {}
            _ => return Err(AuthError::UnsupportedAlgorithm),
        }

        // Every token must reference the current key. When we add
        // key rotation, this becomes a lookup against a key registry.
        // Reject tokens that omit `kid` entirely — the issuer always
        // sets it (see TokenIssuer::sign_*), and accepting kid-less
        // tokens would let an attacker who somehow forged a signature
        // bypass the key-pinning invariant.
        match &header.kid {
            Some(kid) if kid == &self.key.kid => {}
            Some(_) | None => return Err(AuthError::UnknownKey),
        }

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.expected_issuer]);
        validation.set_audience(&[&self.expected_audience]);
        validation.leeway = self.leeway_seconds;

        let data = decode::<Claims>(token, &self.key.decoding, &validation)
            .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

        Ok(data.claims)
    }
}

// -- Extractor -------------------------------------------------------

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AuthError::MissingToken)?;

        let verifier = parts
            .extensions
            .get::<Arc<AuthVerifier>>()
            .cloned()
            .expect("AuthVerifier extension not installed");

        let claims = verifier.verify(bearer.token())?;
        let user: AuthenticatedUser = claims.into();

        // Device tokens are subject to server-side revocation: the
        // signed JWT is valid for 90 days, but the user can hit
        // DELETE /v1/auth/devices/:id to invalidate it sooner. The
        // DeviceStore extension lets us turn that revocation into an
        // immediate 401.
        if matches!(user.token_type, TokenType::Device) {
            let device_id = user.device_id.ok_or(AuthError::InvalidToken(
                "device token without device_id claim".into(),
            ))?;
            let store = parts
                .extensions
                .get::<Arc<dyn crate::devices::DeviceStore>>()
                .cloned()
                .expect("DeviceStore extension not installed");
            match store.is_device_active(device_id).await {
                Ok(true) => {}
                Ok(false) => return Err(AuthError::InvalidToken("device revoked".into())),
                Err(e) => {
                    tracing::error!(error = %e, "device revocation lookup failed");
                    return Err(AuthError::InvalidToken("device check failed".into()));
                }
            }
        }

        Ok(user)
    }
}

// -- Test support + tests -------------------------------------------

#[cfg(test)]
pub mod test_support {
    //! Helpers for the test suites elsewhere in the crate.

    use super::*;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::RsaPrivateKey;

    pub const TEST_ISSUER: &str = "https://stats.example.com";
    pub const TEST_AUDIENCE: &str = "starstats";

    /// Build an issuer + verifier pair backed by a fresh in-memory
    /// keypair. Returns (issuer, verifier).
    pub fn fresh_pair() -> (TokenIssuer, AuthVerifier) {
        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 2048).expect("rsa keygen");
        let pem = priv_key
            .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
            .expect("pkcs1 pem");
        let encoding = EncodingKey::from_rsa_pem(pem.as_bytes()).expect("enc");
        let decoding = decoding_from_private_pem(&pem).expect("dec");
        let key = ServerKey {
            kid: "test-k1".into(),
            encoding: Arc::new(encoding),
            decoding: Arc::new(decoding),
        };
        let issuer = TokenIssuer::new(key.clone(), TEST_ISSUER.into(), TEST_AUDIENCE.into());
        let verifier = AuthVerifier::new(key, TEST_ISSUER.into(), TEST_AUDIENCE.into());
        (issuer, verifier)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn round_trips_user_token() {
        let (issuer, verifier) = fresh_pair();
        let token = issuer.sign_user("user-123", "TheCodeSaiyan").unwrap();
        let claims = verifier.verify(&token).unwrap();
        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.preferred_username, "TheCodeSaiyan");
        assert_eq!(claims.token_type, TokenType::User);
    }

    #[test]
    fn round_trips_device_token() {
        let (issuer, verifier) = fresh_pair();
        let device_id = Uuid::new_v4();
        let token = issuer
            .sign_device("user-123", "Daisy's PC", device_id)
            .unwrap();
        let claims = verifier.verify(&token).unwrap();
        assert_eq!(claims.token_type, TokenType::Device);
        assert_eq!(claims.preferred_username, "Daisy's PC");
        assert_eq!(claims.device_id, Some(device_id));
    }

    #[test]
    fn rejects_token_signed_by_other_key() {
        let (issuer, _) = fresh_pair();
        let (_, verifier) = fresh_pair();
        let token = issuer.sign_user("user-123", "x").unwrap();
        let err = verifier.verify(&token).unwrap_err();
        // The kid won't match; signature would also fail.
        assert!(matches!(
            err,
            AuthError::UnknownKey | AuthError::InvalidToken(_)
        ));
    }

    #[test]
    fn rejects_hs256_outright() {
        use jsonwebtoken::{encode, Header};
        let (_, verifier) = fresh_pair();
        let bogus_secret = b"shared-secret";
        let token = encode(
            &Header::new(Algorithm::HS256),
            &serde_json::json!({
                "sub": "user-x",
                "iss": TEST_ISSUER,
                "aud": TEST_AUDIENCE,
                "exp": 9999999999_u64,
            }),
            &EncodingKey::from_secret(bogus_secret),
        )
        .unwrap();
        let err = verifier.verify(&token).unwrap_err();
        assert!(matches!(err, AuthError::UnsupportedAlgorithm));
    }

    #[test]
    fn rejects_wrong_audience() {
        let (issuer, _) = fresh_pair();
        let key = test_only_key_from_issuer(&issuer);
        let bad_verifier = AuthVerifier::new(key, TEST_ISSUER.into(), "wrong-audience".into());
        let token = issuer.sign_user("u", "x").unwrap();
        let err = bad_verifier.verify(&token).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    /// Pull the ServerKey out of an issuer for tests where we need to
    /// build a verifier with mismatched expectations.
    fn test_only_key_from_issuer(issuer: &TokenIssuer) -> ServerKey {
        issuer.key.clone()
    }
}
