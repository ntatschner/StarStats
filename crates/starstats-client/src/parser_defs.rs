//! Runtime parser-definition cache.
//!
//! Owns:
//!   1. the active list of compiled remote rules, behind a `RwLock`
//!      so the gamelog hot path reads them without contention;
//!   2. a periodic fetcher that polls
//!      `GET <api_url>/v1/parser-definitions` every 6h, writes the
//!      manifest into SQLite, and swaps in the freshly-compiled
//!      rules.
//!
//! Architectural rule: the gamelog ingest never blocks on the network.
//! On startup the cache is loaded synchronously from SQLite, the
//! ingest worker takes a clone of the `RwLock<Arc<...>>` reader, and
//! the network fetcher writes through to both layers in the
//! background.

use crate::storage::Storage;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use starstats_core::{compile_rules, CompiledRemoteRule, Manifest};
use std::sync::Arc;
use std::time::Duration;

const FETCH_INTERVAL: Duration = Duration::from_secs(6 * 3600);
const FETCH_PATH: &str = "/v1/parser-definitions";

/// Active rules, swapped atomically when a fresh manifest lands.
/// Wrapped in an `Arc` so the gamelog worker can hold a reader without
/// keeping the lock for the duration of a tail iteration.
#[derive(Clone, Default)]
pub struct RuleCache {
    inner: Arc<RwLock<Arc<Vec<CompiledRemoteRule>>>>,
}

impl RuleCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of the current rule list. Cheap — just an
    /// `Arc::clone`.
    pub fn snapshot(&self) -> Arc<Vec<CompiledRemoteRule>> {
        self.inner.read().clone()
    }

    fn replace(&self, rules: Vec<CompiledRemoteRule>) {
        *self.inner.write() = Arc::new(rules);
    }
}

/// Hydrate the cache from SQLite. Call this once at startup, before
/// ingest spawns. Failures are logged but non-fatal — first-run users
/// will simply have no rules until the network fetch lands.
pub fn hydrate_from_storage(storage: &Storage, cache: &RuleCache) {
    match storage.read_parser_def_manifest() {
        Ok(Some(payload)) => match serde_json::from_str::<Manifest>(&payload) {
            Ok(manifest) => {
                let (compiled, errors) = compile_rules(&manifest.rules);
                if !errors.is_empty() {
                    tracing::warn!(
                        rule_errors = errors.len(),
                        first = ?errors.first(),
                        "some cached parser-def rules failed to compile"
                    );
                }
                tracing::info!(
                    rules = compiled.len(),
                    manifest_version = manifest.version,
                    "hydrated parser-def cache from sqlite"
                );
                cache.replace(compiled);
            }
            Err(e) => {
                tracing::warn!(error = %e, "cached parser-def manifest is unparseable; ignoring");
            }
        },
        Ok(None) => {
            tracing::debug!("no cached parser-def manifest yet (first run)");
        }
        Err(e) => {
            tracing::warn!(error = %e, "read_parser_def_manifest failed; ignoring");
        }
    }
}

/// Background fetcher loop. Polls every 6h. The first iteration runs
/// immediately so a cold-start client picks up the active manifest
/// without waiting a quarter of a day.
pub async fn run_fetcher(api_url: String, storage: Arc<Storage>, cache: RuleCache) {
    loop {
        match fetch_once(&api_url, &storage, &cache).await {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(error = %e, "parser-defs fetch failed; will retry");
            }
        }
        tokio::time::sleep(FETCH_INTERVAL).await;
    }
}

async fn fetch_once(api_url: &str, storage: &Storage, cache: &RuleCache) -> Result<()> {
    let url = format!("{}{}", api_url.trim_end_matches('/'), FETCH_PATH);
    let client = reqwest::Client::builder()
        .user_agent(concat!("StarStats/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
        .context("build reqwest client")?;
    let resp = client
        .get(&url)
        .send()
        .await
        .context("GET parser-definitions")?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("non-success status {status}");
    }
    let body = resp.text().await.context("read parser-defs body")?;
    let manifest: Manifest = serde_json::from_str(&body).context("parse manifest")?;

    storage
        .write_parser_def_manifest(manifest.version, &body)
        .context("write manifest cache")?;

    let (compiled, errors) = compile_rules(&manifest.rules);
    if !errors.is_empty() {
        tracing::warn!(
            rule_errors = errors.len(),
            first = ?errors.first(),
            "some fetched parser-def rules failed to compile"
        );
    }
    tracing::info!(
        rules = compiled.len(),
        manifest_version = manifest.version,
        "applied fresh parser-def manifest"
    );
    cache.replace(compiled);
    Ok(())
}
