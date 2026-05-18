//! On-disk client configuration and per-platform paths.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted user configuration. Lives at
///  - Windows: `%APPDATA%\StarStats\config.toml`
///  - Linux:   `$XDG_CONFIG_HOME/StarStats/config.toml`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Override the auto-discovered Game.log path.
    pub gamelog_path: Option<PathBuf>,
    /// Sync to the remote StarStats API server.
    pub remote_sync: RemoteSyncConfig,
    /// Web UI origin — used to deep-link the user back to the website
    /// (e.g. for email verification, "Open on web"). When unset, the
    /// effective value is derived from `remote_sync.api_url` by
    /// stripping a leading `api.` from the hostname (see
    /// `Config::effective_web_origin`), so most users don't need to
    /// configure it. Self-hosted setups with a non-`api.` host should
    /// set this explicitly.
    pub web_origin: Option<String>,
    /// Automatically check for updates on startup. Defaults to true;
    /// the Updates card in Settings exposes a toggle. Disabled users
    /// can still trigger a manual check via the same card.
    #[serde(default = "default_auto_update_check")]
    pub auto_update_check: bool,
    /// Which release channel to track. Drives the updater endpoint —
    /// each channel has its own manifest at
    /// `release-manifests/<channel>.json` on the main branch.
    /// Default is derived from `CARGO_PKG_VERSION` at compile time
    /// (alpha/beta/rc/live) so new installs land on the channel the
    /// build itself ships on. Users can opt into a different channel
    /// via the Settings dropdown.
    #[serde(default)]
    pub release_channel: ReleaseChannel,
    /// When true, the tray writes a daily-rolling `client.log` to
    /// the user data dir for diagnostics. Defaults to false to keep
    /// disk use minimal — toggle on from Settings → Updates if you
    /// need to capture logs for a bug report. The panic-only log is
    /// always written regardless of this flag.
    #[serde(default)]
    pub debug_logging: bool,
    /// Visual theme applied to the tray webview. Drives the
    /// `[data-theme="..."]` attribute the design tokens scope against.
    /// Defaults to Stanton (warm amber) — the design system's canonical
    /// dark theme.
    #[serde(default)]
    pub theme: Theme,
    /// Per-user dismissal log for Health items. Permanent (no
    /// expiry); items re-emerge when the underlying params change
    /// (the fingerprint is over (id, params), not (id) alone).
    /// Only `Severity::Warn` and `Severity::Info` items are
    /// dismissible — the rule is enforced Rust-side in `health.rs`.
    #[serde(default)]
    pub dismissed_health: Vec<crate::health::DismissedHealth>,
    /// Stable per-install anonymous ID for parser submissions. The
    /// server uses `(shape_hash, client_anon_id)` as the dedupe key
    /// so repeated submissions from the same install fold into one
    /// row. Lazily generated on first call to
    /// `get_or_create_client_anon_id()` and persisted from then on.
    /// `Option` so existing config.toml files survive the upgrade
    /// without a migration.
    #[serde(default)]
    pub client_anon_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gamelog_path: None,
            remote_sync: RemoteSyncConfig::default(),
            web_origin: None,
            auto_update_check: default_auto_update_check(),
            release_channel: ReleaseChannel::default(),
            debug_logging: false,
            theme: Theme::default(),
            dismissed_health: Vec::new(),
            client_anon_id: None,
        }
    }
}

impl Config {
    /// Resolve the effective web origin for deep-link affordances
    /// (e.g. the tray's "Open on web" button).
    ///
    /// Priority order:
    /// 1. Explicit `web_origin` from config.toml — honoured verbatim.
    /// 2. Derived from `remote_sync.api_url` by stripping a leading
    ///    `api.` from the hostname (e.g. `https://api.starstats.app`
    ///    → `https://starstats.app`). The rewrite preserves scheme,
    ///    port, and path; only the host is touched.
    /// 3. `None` when neither is usable.
    ///
    /// Returning the API URL unmodified is never correct: the API
    /// subdomain serves JSON, not HTML, so `/u/<handle>` 404s. The
    /// old TS-side fallback chain (App.tsx) did exactly that — the
    /// fix is to move the resolution Rust-side so the contract is a
    /// single source of truth.
    pub fn effective_web_origin(&self) -> Option<String> {
        if let Some(origin) = self.web_origin.as_deref() {
            let trimmed = origin.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.trim_end_matches('/').to_string());
            }
        }
        derive_web_origin_from_api_url(self.remote_sync.api_url.as_deref()?)
    }
}

/// Best-effort `api.<rest>` → `<rest>` host rewrite. Returns `None`
/// for unparseable URLs or hosts that don't start with `api.`.
/// Preserves scheme and the host's port suffix; the path is
/// discarded (we want an origin, not a deep link).
///
/// String-parsing rather than a `url::Url` round-trip on purpose —
/// avoids pulling the `url` crate into the client just for this
/// single helper, and the input shape is well-constrained
/// (`scheme://host[:port][/...]`).
fn derive_web_origin_from_api_url(api_url: &str) -> Option<String> {
    let trimmed = api_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let (scheme, rest) = trimmed.split_once("://")?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    // `rest` is `host[:port][/path][?query]`. Strip the path/query
    // first so a path segment containing `api.` can't trip the host
    // rewrite (would-be `https://example.com/api.bar` is left alone).
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let new_authority = authority.strip_prefix("api.")?;
    if new_authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{new_authority}"))
}

/// User-selectable visual theme. Each variant matches one of the four
/// `[data-theme="..."]` blocks in `starstats-tokens.css` — switching
/// themes is just a paint change (no layout reflow, no font swap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Warm amber on charcoal. The design system's default.
    Stanton,
    /// Molten coral, more aggressive accent. Dark.
    Pyro,
    /// Cool teal, clinical. Dark.
    Terra,
    /// Deep violet on warm off-white. Light.
    Nyx,
}

impl Default for Theme {
    fn default() -> Self {
        Self::Stanton
    }
}

impl Theme {
    /// Lowercase token serialised into config.toml and matched by the
    /// `[data-theme="..."]` selectors in `starstats-tokens.css`.
    /// Currently unused (serde's `rename_all = "snake_case"` produces
    /// the same string for the persistence path), but kept on the
    /// public API for callers that need the literal token without a
    /// `serde_json::to_value` round-trip.
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Theme::Stanton => "stanton",
            Theme::Pyro => "pyro",
            Theme::Terra => "terra",
            Theme::Nyx => "nyx",
        }
    }
}

/// User-selectable release channel. Each channel maps to a stable
/// manifest URL on the `main` branch; the release workflow writes the
/// generated manifest into `release-manifests/<channel>.json` based
/// on the tag's pre-release suffix.
///
/// Switching channels changes which manifest the updater queries on
/// next check — no reinstall required. The Tauri updater only offers
/// a download when the manifest version is strictly greater than the
/// installed version (semver), so switching from Beta to Live while
/// running a newer prerelease will not roll back; you'll simply
/// receive nothing until Live catches up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    /// Pre-release alpha builds — `vX.Y.Z-alpha[.N]`. Retained as an
    /// opt-in channel; no longer the default channel for fresh installs
    /// (see `Default` impl).
    Alpha,
    /// Beta builds — `vX.Y.Z-beta[.N]`. The active pre-release channel
    /// post-history-scrub.
    Beta,
    /// Release candidates — `vX.Y.Z-rc[.N]`. Intended for users who
    /// want stability ahead of GA but accept the occasional regression.
    Rc,
    /// Stable releases — bare `vX.Y.Z` tags. The conservative default
    /// once the project hits 1.0; for now this channel is empty.
    Live,
}

impl Default for ReleaseChannel {
    /// Derive the default channel from this build's package version so
    /// the installer always lands on the channel it ships on. A user
    /// who installs `v0.0.1-beta` defaults to Beta; the future first
    /// `vX.Y.Z` release defaults to Live, etc. Persisted user overrides
    /// (config.toml) still win over this default.
    fn default() -> Self {
        Self::from_version(env!("CARGO_PKG_VERSION"))
    }
}

impl ReleaseChannel {
    /// Lowercase token used in the manifest filename and the Settings
    /// dropdown's serialised value.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReleaseChannel::Alpha => "alpha",
            ReleaseChannel::Beta => "beta",
            ReleaseChannel::Rc => "rc",
            ReleaseChannel::Live => "live",
        }
    }

    /// Map a semver string to a channel by inspecting its prerelease
    /// suffix. Anything without a recognised suffix is treated as Live
    /// (the conservative choice for unrecognised inputs).
    pub fn from_version(v: &str) -> Self {
        let Some((_, suffix)) = v.split_once('-') else {
            return Self::Live;
        };
        // suffix may be "alpha", "alpha.1", "beta.2", "rc", etc.
        match suffix.split('.').next().unwrap_or("") {
            "alpha" => Self::Alpha,
            "beta" => Self::Beta,
            "rc" => Self::Rc,
            _ => Self::Live,
        }
    }

    /// Stable updater endpoint for this channel — points at the
    /// manifest on the main branch via raw.githubusercontent.com.
    /// Stable across releases (a single tag's manifest URL would
    /// 404 for prereleases via `/releases/latest/`, which is why
    /// we don't use that anymore).
    pub fn manifest_url(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/ntatschner/StarStats/main/release-manifests/{}.json",
            self.as_str()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_version_maps_known_prerelease_suffixes() {
        assert_eq!(
            ReleaseChannel::from_version("0.0.1-alpha"),
            ReleaseChannel::Alpha
        );
        assert_eq!(
            ReleaseChannel::from_version("0.3.12-alpha.1"),
            ReleaseChannel::Alpha
        );
        assert_eq!(
            ReleaseChannel::from_version("0.0.1-beta"),
            ReleaseChannel::Beta
        );
        assert_eq!(
            ReleaseChannel::from_version("1.0.0-beta.2"),
            ReleaseChannel::Beta
        );
        assert_eq!(ReleaseChannel::from_version("1.0.0-rc"), ReleaseChannel::Rc);
        assert_eq!(
            ReleaseChannel::from_version("1.0.0-rc.4"),
            ReleaseChannel::Rc
        );
    }

    #[test]
    fn from_version_treats_bare_version_as_live() {
        assert_eq!(ReleaseChannel::from_version("1.0.0"), ReleaseChannel::Live);
        assert_eq!(ReleaseChannel::from_version("0.0.1"), ReleaseChannel::Live);
    }

    #[test]
    fn from_version_falls_back_to_live_for_unknown_suffix() {
        // Unknown prerelease tokens are conservative: don't silently
        // accept random text as a real channel.
        assert_eq!(
            ReleaseChannel::from_version("1.0.0-canary"),
            ReleaseChannel::Live
        );
        assert_eq!(ReleaseChannel::from_version("1.0.0-"), ReleaseChannel::Live);
    }

    #[test]
    fn default_tracks_cargo_pkg_version() {
        // Smoke check: the compile-time default must agree with parsing
        // the package version at runtime.
        assert_eq!(
            ReleaseChannel::default(),
            ReleaseChannel::from_version(env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn missing_dismissed_health_defaults_empty() {
        // Simulate a TOML written before the field existed.
        let toml_str = r#"
            gamelog_path = "/tmp/Game.log"
            auto_update_check = false
            release_channel = "alpha"
            debug_logging = false
            theme = "stanton"

            [remote_sync]
            enabled = false
            api_url = "https://api.example"
            claimed_handle = "test"
            access_token = "tok"
            interval_secs = 60
            batch_size = 200
        "#;
        let cfg: Config = toml::from_str(toml_str).expect("parse legacy config");
        assert!(cfg.dismissed_health.is_empty());
    }

    #[test]
    fn dismissed_health_round_trips() {
        let mut cfg = Config::default();
        cfg.dismissed_health.push(crate::health::DismissedHealth {
            id: crate::health::HealthId::UpdateAvailable,
            fingerprint:
                "[\"update_available\",{\"id\":\"update_available\",\"version\":\"0.4.1\"}]".into(),
            dismissed_at: chrono::Utc::now(),
        });
        let s = toml::to_string_pretty(&cfg).expect("serialise");
        let round: Config = toml::from_str(&s).expect("deserialise");
        assert_eq!(round.dismissed_health.len(), 1);
        assert_eq!(
            round.dismissed_health[0].id,
            crate::health::HealthId::UpdateAvailable
        );
    }

    #[test]
    fn as_str_round_trips_through_serde() {
        for c in [
            ReleaseChannel::Alpha,
            ReleaseChannel::Beta,
            ReleaseChannel::Rc,
            ReleaseChannel::Live,
        ] {
            let json = serde_json::to_string(&c).unwrap();
            // serde renders enum variants quoted; strip quotes to compare.
            assert_eq!(json.trim_matches('"'), c.as_str());
        }
    }

    #[test]
    fn theme_default_is_stanton() {
        assert_eq!(Theme::default(), Theme::Stanton);
        assert_eq!(Config::default().theme, Theme::Stanton);
    }

    #[test]
    fn theme_round_trips_through_serde() {
        for t in [Theme::Stanton, Theme::Pyro, Theme::Terra, Theme::Nyx] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(json.trim_matches('"'), t.as_str());
            let parsed: Theme = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, t);
        }
    }

    #[test]
    fn config_without_theme_field_deserialises_to_stanton() {
        // Backward-compat: configs persisted before the theme field
        // existed must still load. `#[serde(default)]` on Config
        // covers absent fields by inserting Theme::default().
        let toml_text = "auto_update_check = true\n";
        let cfg: Config = toml::from_str(toml_text).unwrap();
        assert_eq!(cfg.theme, Theme::Stanton);
    }

    #[test]
    fn default_remote_sync_api_url_is_public_origin() {
        let cfg = Config::default();
        assert_eq!(
            cfg.remote_sync.api_url.as_deref(),
            Some(DEFAULT_API_URL),
            "fresh installs should default to the public StarStats API",
        );
    }

    #[test]
    fn config_without_api_url_field_deserialises_to_default() {
        // Backward-compat: configs persisted before the default
        // landed (or without the field set) should inherit the new
        // default via #[serde(default)] on RemoteSyncConfig.
        let toml_text = "[remote_sync]\nenabled = true\n";
        let cfg: Config = toml::from_str(toml_text).unwrap();
        assert_eq!(cfg.remote_sync.api_url.as_deref(), Some(DEFAULT_API_URL));
    }

    // -- effective_web_origin / derive_web_origin_from_api_url -------
    //
    // Regression coverage for the "Open on web takes you to the API
    // subdomain" bug. The TS code used to fall back to api_url raw —
    // the fix moved the rewrite Rust-side so there's one resolution
    // path and one place to test it.

    #[test]
    fn effective_web_origin_prefers_explicit_value() {
        let cfg = Config {
            web_origin: Some("https://custom.example".to_string()),
            remote_sync: RemoteSyncConfig {
                api_url: Some("https://api.starstats.app".to_string()),
                ..RemoteSyncConfig::default()
            },
            ..Config::default()
        };
        assert_eq!(
            cfg.effective_web_origin().as_deref(),
            Some("https://custom.example")
        );
    }

    #[test]
    fn effective_web_origin_strips_trailing_slashes_from_explicit_value() {
        let cfg = Config {
            web_origin: Some("https://custom.example///".to_string()),
            ..Config::default()
        };
        assert_eq!(
            cfg.effective_web_origin().as_deref(),
            Some("https://custom.example")
        );
    }

    #[test]
    fn effective_web_origin_treats_blank_explicit_value_as_unset() {
        let cfg = Config {
            web_origin: Some("   ".to_string()),
            remote_sync: RemoteSyncConfig {
                api_url: Some("https://api.starstats.app".to_string()),
                ..RemoteSyncConfig::default()
            },
            ..Config::default()
        };
        assert_eq!(
            cfg.effective_web_origin().as_deref(),
            Some("https://starstats.app")
        );
    }

    #[test]
    fn effective_web_origin_derives_from_api_url_when_unset() {
        let cfg = Config::default(); // ships DEFAULT_API_URL
        assert_eq!(
            cfg.effective_web_origin().as_deref(),
            Some("https://starstats.app")
        );
    }

    #[test]
    fn effective_web_origin_returns_none_when_both_unset() {
        let cfg = Config {
            remote_sync: RemoteSyncConfig {
                api_url: None,
                ..RemoteSyncConfig::default()
            },
            ..Config::default()
        };
        assert!(cfg.effective_web_origin().is_none());
    }

    #[test]
    fn derive_web_origin_strips_api_prefix_from_hostname() {
        assert_eq!(
            derive_web_origin_from_api_url("https://api.starstats.app"),
            Some("https://starstats.app".to_string())
        );
        assert_eq!(
            derive_web_origin_from_api_url("https://api.starstats.app/"),
            Some("https://starstats.app".to_string())
        );
    }

    #[test]
    fn derive_web_origin_preserves_scheme_and_port() {
        assert_eq!(
            derive_web_origin_from_api_url("http://api.example.test:8080/v1"),
            Some("http://example.test:8080".to_string())
        );
    }

    #[test]
    fn derive_web_origin_discards_path() {
        // Origin = scheme + authority; the deep-link path comes from
        // the caller, never from the api_url.
        assert_eq!(
            derive_web_origin_from_api_url("https://api.starstats.app/v1/healthz"),
            Some("https://starstats.app".to_string())
        );
    }

    #[test]
    fn derive_web_origin_ignores_api_in_path_segment() {
        // The `api.` prefix only counts on the HOST, not anywhere in
        // the URL. A user pointing api_url at a path under a non-api
        // host should not silently rewrite.
        assert_eq!(
            derive_web_origin_from_api_url("https://example.com/api.bar"),
            None
        );
    }

    #[test]
    fn derive_web_origin_returns_none_for_hosts_without_api_prefix() {
        // Self-hosted users on `localhost`, raw IPs, or custom
        // hostnames get None — the rewrite is best-effort, not magical.
        // The "Open on web" affordance renders disabled in that case.
        assert_eq!(
            derive_web_origin_from_api_url("http://localhost:8080"),
            None
        );
        assert_eq!(
            derive_web_origin_from_api_url("http://127.0.0.1:3000"),
            None
        );
        assert_eq!(derive_web_origin_from_api_url("https://example.com"), None);
    }

    #[test]
    fn derive_web_origin_rejects_malformed_inputs() {
        assert_eq!(derive_web_origin_from_api_url(""), None);
        assert_eq!(derive_web_origin_from_api_url("   "), None);
        assert_eq!(derive_web_origin_from_api_url("not-a-url"), None);
        assert_eq!(derive_web_origin_from_api_url("://no-scheme.example"), None);
        // host == "api." with nothing after → would yield empty
        // authority; the helper returns None rather than handing back
        // `https://`.
        assert_eq!(derive_web_origin_from_api_url("https://api."), None);
    }

    #[test]
    fn config_preserves_custom_api_url() {
        // A user pointing at a self-hosted / dev instance keeps their
        // URL; the default only applies when the field is absent.
        let toml_text = r#"
            [remote_sync]
            enabled = true
            api_url = "http://localhost:8080"
        "#;
        let cfg: Config = toml::from_str(toml_text).unwrap();
        assert_eq!(
            cfg.remote_sync.api_url.as_deref(),
            Some("http://localhost:8080")
        );
    }
}

fn default_auto_update_check() -> bool {
    true
}

/// Public production StarStats API origin. Used as the default
/// `RemoteSyncConfig.api_url` so a fresh install can hit Enable and
/// proceed straight to pairing without first hunting down a URL.
/// Users on self-hosted instances override via Settings.
pub const DEFAULT_API_URL: &str = "https://api.starstats.app";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteSyncConfig {
    pub enabled: bool,
    /// Base URL of the StarStats API. Defaults to the public
    /// production origin (`DEFAULT_API_URL`). Override to point at
    /// a self-hosted server or a local dev instance.
    pub api_url: Option<String>,
    /// RSI handle the user claims. Server cross-checks this against
    /// the bearer token's `preferred_username`; mismatch → 403.
    pub claimed_handle: Option<String>,
    /// Bearer token issued by the StarStats API. The user pastes one
    /// in for now; Slice 3 of the auth migration replaces this with a
    /// device-pairing flow driven from the website.
    pub access_token: Option<String>,
    /// How often to drain unsent events. Default 60 s.
    #[serde(default = "default_sync_interval_secs")]
    pub interval_secs: u64,
    /// Max events per batch. Above this we split — server caps batch
    /// size and we get clean partial-success accounting.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

impl Default for RemoteSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: Some(DEFAULT_API_URL.to_string()),
            claimed_handle: None,
            access_token: None,
            interval_secs: default_sync_interval_secs(),
            batch_size: default_batch_size(),
        }
    }
}

fn default_sync_interval_secs() -> u64 {
    60
}

fn default_batch_size() -> usize {
    200
}

fn project_dirs() -> Result<directories::ProjectDirs> {
    directories::ProjectDirs::from("app", "StarStats", "tray")
        .context("could not resolve user config/data directories")
}

pub fn config_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&dir).context("create config dir")?;
    Ok(dir)
}

pub fn data_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir).context("create data dir")?;
    Ok(dir)
}

pub fn load() -> Result<Config> {
    let path = config_dir()?.join("config.toml");
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(&path).context("read config.toml")?;
    let cfg: Config = toml::from_str(&text).context("parse config.toml")?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_dir()?.join("config.toml");
    let text = toml::to_string_pretty(cfg).context("serialise config")?;
    std::fs::write(&path, text).context("write config.toml")?;
    Ok(())
}
