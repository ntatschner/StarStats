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
    /// (e.g. for email verification). Falls back to `api_url` (with a
    /// best-effort `api.` → `app.` rewrite, if applicable) when unset
    /// so most users don't need to configure it.
    pub web_origin: Option<String>,
    /// Automatically check for updates on startup. Defaults to true;
    /// the Updates card in Settings exposes a toggle. Disabled users
    /// can still trigger a manual check via the same card.
    #[serde(default = "default_auto_update_check")]
    pub auto_update_check: bool,
    /// Which release channel to track. Drives the updater endpoint —
    /// each channel has its own manifest at
    /// `release-manifests/<channel>.json` on the main branch.
    /// Default is Alpha while we're pre-1.0; users can opt into RC or
    /// Live via the Settings dropdown.
    #[serde(default)]
    pub release_channel: ReleaseChannel,
    /// When true, the tray writes a daily-rolling `client.log` to
    /// the user data dir for diagnostics. Defaults to false to keep
    /// disk use minimal — toggle on from Settings → Updates if you
    /// need to capture logs for a bug report. The panic-only log is
    /// always written regardless of this flag.
    #[serde(default)]
    pub debug_logging: bool,
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
/// installed version (semver), so switching from Alpha to Live while
/// running a newer prerelease will not roll back; you'll simply
/// receive nothing until Live catches up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    /// Pre-release builds — anything tagged `vX.Y.Z-alpha[.N]`.
    /// Default while the project is pre-1.0 because that's the only
    /// channel currently producing builds.
    #[default]
    Alpha,
    /// Release candidates — `vX.Y.Z-rc[.N]`. Intended for users who
    /// want stability ahead of GA but accept the occasional regression.
    Rc,
    /// Stable releases — bare `vX.Y.Z` tags. The conservative default
    /// once the project hits 1.0; for now this channel is empty.
    Live,
}

impl ReleaseChannel {
    /// Lowercase token used in the manifest filename and the Settings
    /// dropdown's serialised value.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReleaseChannel::Alpha => "alpha",
            ReleaseChannel::Rc => "rc",
            ReleaseChannel::Live => "live",
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

fn default_auto_update_check() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RemoteSyncConfig {
    pub enabled: bool,
    /// Base URL of the StarStats API, e.g. `https://api.example.com`.
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
