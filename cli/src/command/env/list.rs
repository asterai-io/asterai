use crate::artifact::ArtifactSyncTag;
use crate::auth::Auth;
use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use asterai_runtime::environment::Environment;
use eyre::Context;
use semver::Version;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEnvironmentsResponse {
    environments: Vec<EnvironmentSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentSummary {
    namespace: String,
    name: String,
    latest_version: String,
}

/// A single entry from the environment list.
#[derive(Debug, Clone)]
pub struct EnvListEntry {
    pub namespace: String,
    pub name: String,
    pub version: Option<String>,
    pub remote_version: Option<String>,
    pub component_count: usize,
    pub sync_tag: ArtifactSyncTag,
}

impl EnvArgs {
    pub async fn list(&self) -> eyre::Result<()> {
        let entries = self.list_entries().await?;
        println!("environments:");
        if entries.is_empty() {
            println!("  (none)");
            return Ok(());
        }
        for entry in &entries {
            let display_ver = entry.version.as_deref().or(entry.remote_version.as_deref());
            let ref_str = match display_ver {
                Some(v) => format!("{}:{}@{}", entry.namespace, entry.name, v),
                None => format!("{}:{}", entry.namespace, entry.name),
            };
            if entry.component_count > 0 {
                println!(
                    "  {}  ({} components)  [{}]",
                    ref_str, entry.component_count, entry.sync_tag
                );
            } else {
                println!("  {}  [{}]", ref_str, entry.sync_tag);
            }
        }
        Ok(())
    }

    pub async fn list_entries(&self) -> eyre::Result<Vec<EnvListEntry>> {
        // Dedup local envs by namespace:name, keeping the highest version.
        let all_local = LocalStore::list_environments();
        let mut local_map: HashMap<String, Environment> = HashMap::new();
        for env in all_local {
            let id = format!("{}:{}", env.namespace(), env.name());
            let dominated = match local_map.get(&id) {
                None => true,
                Some(prev) => {
                    let cur = Version::parse(env.version()).unwrap_or(Version::new(0, 0, 0));
                    let old = Version::parse(prev.version()).unwrap_or(Version::new(0, 0, 0));
                    cur > old
                }
            };
            if dominated {
                local_map.insert(id, env);
            }
        }
        let local_envs: Vec<_> = local_map.into_values().collect();
        let local_refs: HashSet<String> = local_envs
            .iter()
            .map(|e| format!("{}:{}", e.namespace(), e.name()))
            .collect();
        let remote_result = if let Some(api_key) = Auth::read_stored_api_key() {
            fetch_remote_environments(&api_key, &self.api_endpoint).await
        } else {
            Err(eyre::eyre!("not authenticated"))
        };
        let remote_versions: HashMap<String, String> = match &remote_result {
            Ok(remote) => remote
                .iter()
                .map(|e| (format!("{}:{}", e.namespace, e.name), e.latest_version.clone()))
                .collect(),
            Err(_) => HashMap::new(),
        };
        let mut entries = Vec::new();
        for env in &local_envs {
            let id = format!("{}:{}", env.namespace(), env.name());
            let tag = match remote_versions.get(&id) {
                None => ArtifactSyncTag::Unpushed,
                Some(_) if env.is_local() => ArtifactSyncTag::Unpushed,
                Some(remote_ver) => {
                    let local_v = Version::parse(env.version()).unwrap_or(Version::new(0, 0, 0));
                    let remote_v = Version::parse(remote_ver).unwrap_or(Version::new(0, 0, 0));
                    if local_v >= remote_v {
                        ArtifactSyncTag::Synced
                    } else {
                        ArtifactSyncTag::Behind
                    }
                }
            };
            entries.push(EnvListEntry {
                namespace: env.namespace().to_string(),
                name: env.name().to_string(),
                version: Some(env.version().to_string()),
                remote_version: remote_versions.get(&id).cloned(),
                component_count: env.components.len(),
                sync_tag: tag,
            });
        }
        if let Ok(remote) = &remote_result {
            for env in remote {
                let id = format!("{}:{}", env.namespace, env.name);
                if !local_refs.contains(&id) {
                    entries.push(EnvListEntry {
                        namespace: env.namespace.clone(),
                        name: env.name.clone(),
                        version: None,
                        remote_version: Some(env.latest_version.clone()),
                        component_count: 0,
                        sync_tag: ArtifactSyncTag::Remote,
                    });
                }
            }
        }
        Ok(entries)
    }
}

async fn fetch_remote_environments(
    api_key: &str,
    api_url: &str,
) -> eyre::Result<Vec<EnvironmentSummary>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/v1/environments", api_url))
        .header("Authorization", api_key.trim())
        .send()
        .await
        .wrap_err("failed to fetch environments")?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        eyre::bail!("{}: {}", status, error_text);
    }
    let result: ListEnvironmentsResponse =
        response.json().await.wrap_err("failed to parse response")?;
    Ok(result.environments)
}
