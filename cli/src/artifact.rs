use eyre::{Context, bail, eyre};
use reqwest::Response;
use semver::Version;
use serde::Deserialize;
use std::collections::HashMap;
use strum_macros::{Display, EnumString};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub namespace: String,
    pub name: String,
    pub latest_version: String,
}

/// Sync status tag for artifacts (components and environments).
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, Display)]
#[strum(serialize_all = "lowercase")]
pub enum ArtifactSyncTag {
    /// Exists locally but not pushed to remote.
    Unpushed,
    /// Exists both locally and on remote at the same version.
    Synced,
    /// Exists locally but remote has a newer version.
    Behind,
    /// Exists only on remote, not cached locally.
    Remote,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEnvironmentsResponse {
    environments: Vec<ArtifactSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListComponentsResponse {
    pub components: Vec<ArtifactSummary>,
}

impl ArtifactSyncTag {
    /// Determine sync status from local and remote version strings.
    /// A local version of "0.0.0" is treated as unpushed (never published).
    pub fn resolve(local_version: Option<&str>, remote_version: Option<&str>) -> Self {
        match (local_version, remote_version) {
            (None, Some(_)) => Self::Remote,
            (Some(_), None) => Self::Unpushed,
            (None, None) => Self::Unpushed,
            (Some("0.0.0"), Some(_)) => Self::Unpushed,
            (Some(local), Some(remote)) => {
                let local_v = Version::parse(local).unwrap_or(Version::new(0, 0, 0));
                let remote_v = Version::parse(remote).unwrap_or(Version::new(0, 0, 0));
                match local_v >= remote_v {
                    true => Self::Synced,
                    false => Self::Behind,
                }
            }
        }
    }
}

impl ArtifactSummary {
    pub async fn fetch_remote_components(
        api_key: &str,
        api_url: &str,
    ) -> eyre::Result<Vec<ArtifactSummary>> {
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/v1/components", api_url))
            .header("Authorization", api_key.trim())
            .send()
            .await
            .wrap_err("failed to fetch components")?;
        if !response.status().is_success() {
            bail!(get_response_error(response).await);
        }
        let result: ListComponentsResponse =
            response.json().await.wrap_err("failed to parse response")?;
        Ok(result.components)
    }

    pub async fn fetch_remote_environments(
        api_key: &str,
        api_url: &str,
    ) -> eyre::Result<Vec<ArtifactSummary>> {
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/v1/environments", api_url))
            .header("Authorization", api_key.trim())
            .send()
            .await
            .wrap_err("failed to fetch environments")?;
        if !response.status().is_success() {
            bail!(get_response_error(response).await);
        }
        let result: ListEnvironmentsResponse =
            response.json().await.wrap_err("failed to parse response")?;
        Ok(result.environments)
    }

    /// Build a namespace:name -> latest_version lookup from remote summaries.
    pub fn remote_version_map<'a>(
        items: impl Iterator<Item = &'a Self>,
    ) -> HashMap<String, &'a str> {
        items
            .map(|summary| {
                (
                    format!("{}:{}", summary.namespace, summary.name),
                    summary.latest_version.as_str(),
                )
            })
            .collect()
    }
}

async fn get_response_error(response: Response) -> eyre::Report {
    let status = response.status();
    let error_text = response
        .text()
        .await
        .unwrap_or_else(|_| "unknown error".to_string());
    eyre!("{}: {}", status, error_text)
}
