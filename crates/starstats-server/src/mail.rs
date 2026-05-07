//! Transactional email transport.
//!
//! Two implementations of the [`Mailer`] trait:
//!
//!  * [`LettreMailer`] — async SMTP via `lettre`. Built once at startup
//!    from [`SmtpConfig`] and shared via `Arc<dyn Mailer>` so handlers
//!    don't reconnect per request. The transport reuses TLS connections
//!    internally; we treat `send` as fire-and-forget from the caller's
//!    perspective.
//!
//!  * [`NoopMailer`] — used when `SMTP_URL` isn't configured. Logs the
//!    intended send and returns `Ok(())`, which lets local dev (and the
//!    test suite) exercise the full signup → verify path without
//!    standing up a real SMTP server.
//!
//! The verification link format is `${web_origin}/auth/verify?token=…`.
//! That URL is rendered by the Next.js `app/auth/verify/page.tsx`
//! server component, which calls back into `POST /v1/auth/email/verify`
//! with the token from the query string.
//!
//! Errors are surfaced as `anyhow::Error` because the caller (signup)
//! treats every failure here as best-effort — a warn-and-continue path
//! rather than a 500 to the user.

use anyhow::{Context, Result};
use async_trait::async_trait;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::sync::Arc;

use crate::config::SmtpConfig;

/// Pluggable mailer interface so handlers can be parameterised over
/// "send" without dragging in a real SMTP transport in tests.
#[async_trait]
pub trait Mailer: Send + Sync + 'static {
    /// Send the verification email for `to_addr`. `to_name` is the
    /// recipient's display name (we use the claimed RSI handle —
    /// it's the most user-recognisable identifier we have at signup).
    async fn send_verification(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()>;

    /// Send the password-reset email. Same shape as verification —
    /// the link lives on the web app and posts the token back to the
    /// reset-complete endpoint.
    async fn send_password_reset(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()>;

    /// Send a "confirm your new email address" link to the *new*
    /// address while the old address remains the login until the
    /// link is clicked. `to_name` is the user's claimed handle.
    async fn send_email_change_verify(
        &self,
        to_addr: &str,
        to_name: &str,
        token: &str,
    ) -> Result<()>;

    /// Send a one-shot magic-link sign-in. Same shape as the
    /// password-reset flow but the redeemer gets a session JWT
    /// directly instead of bumping a hash.
    async fn send_magic_link(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()>;
}

// -- Lettre (real SMTP) ----------------------------------------------

/// Async SMTP mailer wrapping `lettre::AsyncSmtpTransport`.
///
/// Construction parses `SMTP_URL` once and persists the resulting
/// transport. Lettre internally handles connection pooling and TLS;
/// the caller just hands it a `Message`.
pub struct LettreMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from_addr: String,
    from_name: String,
    web_origin: String,
}

impl LettreMailer {
    /// Build a `LettreMailer` from resolved config. Returns an error
    /// only when the URL fails to parse — there's no network probe at
    /// construction time, since Lettre opens connections lazily on
    /// first send.
    pub fn from_config(cfg: &SmtpConfig) -> Result<Self> {
        let transport =
            build_transport(&cfg.url).with_context(|| format!("parse SMTP_URL `{}`", cfg.url))?;
        Ok(Self {
            transport,
            from_addr: cfg.from_addr.clone(),
            from_name: cfg.from_name.clone(),
            web_origin: cfg.web_origin.trim_end_matches('/').to_string(),
        })
    }
}

/// Parsed pieces of an SMTP URL. Avoids a `url` crate dependency by
/// hand-rolling the small, well-defined subset we accept:
/// `smtp[s]://[user[:pass]@]host[:port]`.
struct ParsedSmtpUrl {
    secure: bool,
    host: String,
    port: u16,
    username: String,
    password: String,
}

fn parse_smtp_url(url: &str) -> Result<ParsedSmtpUrl> {
    let (scheme, rest) = url
        .split_once("://")
        .with_context(|| format!("SMTP_URL missing scheme: `{url}`"))?;
    let secure = match scheme {
        "smtps" => true,
        "smtp" => false,
        other => anyhow::bail!("SMTP_URL has unsupported scheme `{other}`"),
    };

    // Optional userinfo before the last '@'. Use rsplit so passwords
    // containing '@' (rare but legal) don't break parsing.
    let (userinfo, host_and_port) = match rest.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, rest),
    };

    let (host, port) = match host_and_port.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p
                .parse()
                .with_context(|| format!("SMTP_URL port `{p}` is not a number"))?;
            (h.to_string(), port)
        }
        None => (host_and_port.to_string(), if secure { 465 } else { 587 }),
    };
    if host.is_empty() {
        anyhow::bail!("SMTP_URL missing host: `{url}`");
    }

    let (username, password) = match userinfo {
        None => (String::new(), String::new()),
        Some(ui) => match ui.split_once(':') {
            Some((u, p)) => (u.to_string(), p.to_string()),
            None => (ui.to_string(), String::new()),
        },
    };

    Ok(ParsedSmtpUrl {
        secure,
        host,
        port,
        username,
        password,
    })
}

/// Build a transport from the URL. `smtps://` -> implicit TLS (465);
/// `smtp://` -> STARTTLS (587). Lettre dials lazily on the first send.
fn build_transport(url: &str) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let parsed = parse_smtp_url(url)?;
    let mut builder = if parsed.secure {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&parsed.host)?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&parsed.host)?
    };
    builder = builder.port(parsed.port);
    if !parsed.username.is_empty() {
        builder = builder.credentials(Credentials::new(parsed.username, parsed.password));
    }
    Ok(builder.build())
}

#[async_trait]
impl Mailer for LettreMailer {
    async fn send_verification(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()> {
        self.send(
            to_addr,
            to_name,
            "Verify your StarStats email",
            render_verification_body(&self.web_origin, token),
        )
        .await
    }

    async fn send_password_reset(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()> {
        self.send(
            to_addr,
            to_name,
            "Reset your StarStats password",
            render_password_reset_body(&self.web_origin, token),
        )
        .await
    }

    async fn send_email_change_verify(
        &self,
        to_addr: &str,
        to_name: &str,
        token: &str,
    ) -> Result<()> {
        self.send(
            to_addr,
            to_name,
            "Confirm your new StarStats email",
            render_email_change_body(&self.web_origin, token),
        )
        .await
    }

    async fn send_magic_link(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()> {
        self.send(
            to_addr,
            to_name,
            "Sign in to StarStats",
            render_magic_link_body(&self.web_origin, token),
        )
        .await
    }
}

impl LettreMailer {
    /// Shared envelope construction so the three send_* paths don't
    /// re-implement From/To header parsing or the SMTP send call.
    async fn send(&self, to_addr: &str, to_name: &str, subject: &str, body: String) -> Result<()> {
        let from = format!("{} <{}>", self.from_name, self.from_addr);
        let to = format!("{to_name} <{to_addr}>");
        let msg = Message::builder()
            .from(from.parse().context("parse From address")?)
            .to(to.parse().context("parse To address")?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .context("build message")?;
        self.transport.send(msg).await.context("SMTP send failed")?;
        Ok(())
    }
}

/// Render the plain-text email body. Single template — short, no
/// HTML — because the recipient is a single human and we want this to
/// render identically in every client.
fn render_verification_body(web_origin: &str, token: &str) -> String {
    let link = format!("{web_origin}/auth/verify?token={token}");
    format!(
        "Welcome to StarStats!\n\
         \n\
         Click the link below to confirm your email address:\n\
         \n\
         {link}\n\
         \n\
         This link expires in 24 hours. If you didn't sign up, you can\n\
         ignore this message.\n"
    )
}

fn render_password_reset_body(web_origin: &str, token: &str) -> String {
    let link = format!("{web_origin}/auth/reset-password?token={token}");
    format!(
        "Someone (hopefully you) requested a password reset for your\n\
         StarStats account. Click the link below to choose a new\n\
         password:\n\
         \n\
         {link}\n\
         \n\
         This link expires in 30 minutes. If you didn't request a\n\
         reset, you can ignore this message — your password stays\n\
         unchanged.\n\
         \n\
         For your security, all paired devices and active sessions\n\
         will be signed out as soon as the password is changed.\n"
    )
}

fn render_email_change_body(web_origin: &str, token: &str) -> String {
    let link = format!("{web_origin}/auth/email-change?token={token}");
    format!(
        "You asked to change the email address on your StarStats\n\
         account to this one. Click the link below to confirm:\n\
         \n\
         {link}\n\
         \n\
         This link expires in 24 hours. Your old email continues to\n\
         work as your login until you click the link, so a typo here\n\
         won't lock you out — you can simply ignore this message.\n"
    )
}

fn render_magic_link_body(web_origin: &str, token: &str) -> String {
    let link = format!("{web_origin}/auth/magic-link/redeem?token={token}");
    format!(
        "Someone (hopefully you) asked to sign in to StarStats using\n\
         a magic link. Click below to finish signing in:\n\
         \n\
         {link}\n\
         \n\
         This link expires in 15 minutes and can only be used once.\n\
         If you didn't request a sign-in link, you can ignore this\n\
         message — no action will be taken on your account.\n"
    )
}

// -- Noop (no SMTP configured) ---------------------------------------

/// Fallback mailer used when `SMTP_URL` is missing.
///
/// Logs the would-be send at info level — useful in dev where the
/// console doubles as the inbox — and otherwise returns Ok. Never
/// fails, so signup paths can call it unconditionally.
pub struct NoopMailer;

#[async_trait]
impl Mailer for NoopMailer {
    async fn send_verification(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()> {
        tracing::info!(
            to = to_addr,
            name = to_name,
            token,
            "noop mailer: would send verification email"
        );
        Ok(())
    }

    async fn send_password_reset(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()> {
        tracing::info!(
            to = to_addr,
            name = to_name,
            token,
            "noop mailer: would send password reset email"
        );
        Ok(())
    }

    async fn send_email_change_verify(
        &self,
        to_addr: &str,
        to_name: &str,
        token: &str,
    ) -> Result<()> {
        tracing::info!(
            to = to_addr,
            name = to_name,
            token,
            "noop mailer: would send email-change verification"
        );
        Ok(())
    }

    async fn send_magic_link(&self, to_addr: &str, to_name: &str, token: &str) -> Result<()> {
        tracing::info!(
            to = to_addr,
            name = to_name,
            token,
            "noop mailer: would send magic-link"
        );
        Ok(())
    }
}

/// Build the runtime mailer based on config. Always succeeds — a
/// malformed `SMTP_URL` falls back to Noop with a warning, matching
/// the SpiceDB / MinIO degraded-boot posture.
pub fn build_mailer(cfg: Option<&SmtpConfig>) -> Arc<dyn Mailer> {
    match cfg {
        Some(c) => match LettreMailer::from_config(c) {
            Ok(m) => {
                tracing::info!(from = %c.from_addr, "SMTP mailer initialised");
                Arc::new(m)
            }
            Err(e) => {
                tracing::warn!(error = %e, "SMTP init failed; falling back to noop mailer");
                Arc::new(NoopMailer)
            }
        },
        None => {
            tracing::info!("SMTP not configured; using noop mailer (no verification emails)");
            Arc::new(NoopMailer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_mailer_returns_ok() {
        let m = NoopMailer;
        assert!(m
            .send_verification("a@example.com", "Alice", "tok")
            .await
            .is_ok());
    }

    #[test]
    fn render_verification_body_includes_link_and_token() {
        let body = render_verification_body("https://app.example.com", "abc123");
        assert!(body.contains("https://app.example.com/auth/verify?token=abc123"));
        assert!(body.contains("expires in 24 hours"));
    }

    #[test]
    fn render_password_reset_body_includes_link_and_30min_ttl() {
        let body = render_password_reset_body("https://app.example.com", "tok-xyz");
        assert!(body.contains("https://app.example.com/auth/reset-password?token=tok-xyz"));
        assert!(body.contains("30 minutes"));
        assert!(body.contains("paired devices"));
    }

    #[test]
    fn render_email_change_body_includes_link_and_old_email_assurance() {
        let body = render_email_change_body("https://app.example.com", "tok-xyz");
        assert!(body.contains("https://app.example.com/auth/email-change?token=tok-xyz"));
        assert!(body.contains("old email"));
    }

    #[test]
    fn parse_smtp_url_handles_smtps_with_credentials() {
        let p = parse_smtp_url("smtps://user:pa%40ss@smtp.example.com:465").unwrap();
        assert!(p.secure);
        assert_eq!(p.host, "smtp.example.com");
        assert_eq!(p.port, 465);
        assert_eq!(p.username, "user");
        assert_eq!(p.password, "pa%40ss");
    }

    #[test]
    fn parse_smtp_url_defaults_port_for_starttls() {
        let p = parse_smtp_url("smtp://smtp.example.com").unwrap();
        assert!(!p.secure);
        assert_eq!(p.port, 587);
        assert!(p.username.is_empty());
    }

    #[test]
    fn parse_smtp_url_rejects_unknown_scheme() {
        assert!(parse_smtp_url("http://smtp.example.com").is_err());
    }
}
