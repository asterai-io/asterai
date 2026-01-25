use crate::artifact::ArtifactSyncTag;
use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use crate::local_store::LocalStore;
use eyre::Context;
use serde::Deserialize;
use std::collections::HashSet;

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

impl ComponentArgs {
    pub async fn list(&self) -> eyre::Result<()> {
        // Collect local components.
        let local_components = LocalStore::list_components();
        let local_refs: HashSet<String> = local_components
            .iter()
            .map(|c| format!("{}:{}", c.component().namespace(), c.component().name()))
            .collect();
        // Try to fetch remote components.
        let remote_result = if let Some(api_key) = Auth::read_stored_api_key() {
            fetch_remote_components(&api_key, &self.api_endpoint).await
        } else {
            Err(eyre::eyre!("not authenticated"))
        };
        let remote_refs: HashSet<String> = match &remote_result {
            Ok(remote) => remote
                .iter()
                .map(|c| format!("{}:{}", c.namespace, c.name))
                .collect(),
            Err(_) => HashSet::new(),
        };
        // Build unified list.
        let mut entries: Vec<(String, ArtifactSyncTag)> = Vec::new();
        // Add local components.
        for component in &local_components {
            let ref_str = format!(
                "{}:{}@{}",
                component.component().namespace(),
                component.component().name(),
                component.component().version()
            );
            let id = format!(
                "{}:{}",
                component.component().namespace(),
                component.component().name()
            );
            let tag = match remote_refs.contains(&id) {
                true => ArtifactSyncTag::Synced,
                false => ArtifactSyncTag::Unpushed,
            };
            entries.push((ref_str, tag));
        }
        // Add remote-only components.
        if let Ok(remote) = &remote_result {
            for comp in remote {
                let id = format!("{}:{}", comp.namespace, comp.name);
                if !local_refs.contains(&id) {
                    let ref_str =
                        format!("{}:{}@{}", comp.namespace, comp.name, comp.latest_version);
                    entries.push((ref_str, ArtifactSyncTag::Remote));
                }
            }
        }
        println!("components:");
        if entries.is_empty() {
            println!("  (none)");
        } else {
            for (ref_str, tag) in entries {
                println!("  {}  [{}]", ref_str, tag);
            }
        }
        if let Err(e) = &remote_result {
            println!();
            println!("(remote: {})", e);
        }
        Ok(())
    }
}

async fn fetch_remote_components(
    api_key: &str,
    api_url: &str,
) -> eyre::Result<Vec<ComponentSummary>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/v1/components", api_url))
        .header("Authorization", api_key.trim())
        .send()
        .await
        .wrap_err("failed to fetch components")?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        eyre::bail!("{}: {}", status, error_text);
    }
    let result: ListComponentsResponse =
        response.json().await.wrap_err("failed to parse response")?;
    Ok(result.components)
}
