use crate::auth::Auth;
use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use eyre::Context;
use serde::Deserialize;
use std::collections::HashSet;

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

impl EnvArgs {
    pub async fn list(&self) -> eyre::Result<()> {
        // Collect local environments.
        let local_envs = LocalStore::list_environments();
        let local_refs: HashSet<String> = local_envs
            .iter()
            .map(|e| format!("{}:{}", e.namespace(), e.name()))
            .collect();
        // Try to fetch remote environments.
        let remote_result = if let Some(api_key) = Auth::read_stored_api_key() {
            fetch_remote_environments(&api_key, &self.api_endpoint).await
        } else {
            Err(eyre::eyre!("not authenticated"))
        };
        let remote_refs: HashSet<String> = match &remote_result {
            Ok(remote) => remote
                .iter()
                .map(|e| format!("{}:{}", e.namespace, e.name))
                .collect(),
            Err(_) => HashSet::new(),
        };
        // Build unified list.
        let mut entries: Vec<(String, &str, usize)> = Vec::new();
        // Add local environments.
        for env in &local_envs {
            let ref_str = env.resource_ref();
            let id = format!("{}:{}", env.namespace(), env.name());
            let tag = if remote_refs.contains(&id) {
                "locally cached"
            } else {
                "local only"
            };
            entries.push((ref_str, tag, env.components.len()));
        }
        // Add remote-only environments.
        if let Ok(remote) = &remote_result {
            for env in remote {
                let id = format!("{}:{}", env.namespace, env.name);
                if !local_refs.contains(&id) {
                    let ref_str = format!("{}:{}@{}", env.namespace, env.name, env.latest_version);
                    entries.push((ref_str, "remote", 0));
                }
            }
        }
        // Print.
        println!("environments:");
        if entries.is_empty() {
            println!("  (none)");
        } else {
            for (ref_str, tag, component_count) in entries {
                if component_count > 0 {
                    println!("  {}  ({} components)  [{}]", ref_str, component_count, tag);
                } else {
                    println!("  {}  [{}]", ref_str, tag);
                }
            }
        }
        // Show error if remote fetch failed.
        if let Err(e) = &remote_result {
            println!();
            println!("(remote: {})", e);
        }
        Ok(())
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
