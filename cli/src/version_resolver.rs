//! Version resolution for components.
//!
//! When a component version is omitted, this module resolves to the latest
//! known version by checking both local storage and the remote registry.

use crate::auth::Auth;
use crate::local_store::LocalStore;
use eyre::{bail, eyre};
use semver::Version;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListComponentsResponse {
    components: Vec<ComponentSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComponentSummary {
    namespace: String,
    name: String,
    latest_version: String,
}

/// Find the latest local version of a component.
pub fn find_latest_local_version(namespace: &str, name: &str) -> Option<Version> {
    let paths = LocalStore::find_all_versions(namespace, name);
    let mut latest: Option<Version> = None;
    for path in paths {
        let Ok(resource) = LocalStore::resource_from_path(&path) else {
            continue;
        };
        let version = resource.version().clone();
        match &latest {
            None => latest = Some(version),
            Some(current) if version > *current => latest = Some(version),
            _ => {}
        }
    }
    latest
}

/// Fetch the latest remote version of a component from the API.
pub async fn fetch_latest_remote_version(
    namespace: &str,
    name: &str,
    api_endpoint: &str,
) -> eyre::Result<Option<Version>> {
    let Some(api_key) = Auth::read_stored_api_key() else {
        return Ok(None);
    };
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/v1/components", api_endpoint))
        .header("Authorization", api_key.trim())
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let result: ListComponentsResponse = response.json().await?;
    for comp in result.components {
        if comp.namespace == namespace && comp.name == name {
            let version = Version::from_str(&comp.latest_version).ok();
            return Ok(version);
        }
    }
    Ok(None)
}

/// Resolve the latest version of a component from local and remote sources.
/// Returns the highest version found.
pub async fn resolve_latest_version(
    namespace: &str,
    name: &str,
    api_endpoint: &str,
) -> eyre::Result<Version> {
    let local = find_latest_local_version(namespace, name);
    let remote = fetch_latest_remote_version(namespace, name, api_endpoint).await?;
    match (local, remote) {
        (Some(l), Some(r)) => Ok(std::cmp::max(l, r)),
        (Some(l), None) => Ok(l),
        (None, Some(r)) => Ok(r),
        (None, None) => bail!(
            "component {}:{} not found locally or in registry",
            namespace,
            name
        ),
    }
}

/// Represents a component reference with an optional version.
#[derive(Debug)]
pub struct ComponentRef {
    pub namespace: String,
    pub name: String,
    pub version: Option<Version>,
}

impl ComponentRef {
    /// Parse a component reference string.
    /// Accepts: `namespace:name@version`, `namespace:name`, `name@version`, or `name`.
    /// When namespace is omitted, defaults to the logged-in user's namespace.
    pub fn parse(s: &str) -> eyre::Result<Self> {
        let (id_part, version) = match s.split_once('@') {
            Some((id, ver)) => (id, Some(Version::from_str(ver).map_err(|e| eyre!(e))?)),
            None => (s, None),
        };
        let (namespace, name) = match id_part.split_once(':').or_else(|| id_part.split_once('/')) {
            Some((ns, n)) => (ns.to_string(), n.to_string()),
            None => (Auth::read_user_or_fallback_namespace(), id_part.to_string()),
        };
        Ok(Self {
            namespace,
            name,
            version,
        })
    }

    /// Resolve to a full resource string with version.
    /// If version is not specified, resolves to the latest known version.
    pub async fn resolve(&self, api_endpoint: &str) -> eyre::Result<String> {
        let version = match &self.version {
            Some(v) => v.clone(),
            None => resolve_latest_version(&self.namespace, &self.name, api_endpoint).await?,
        };
        Ok(format!("{}:{}@{}", self.namespace, self.name, version))
    }
}
