use crate::artifact::{ArtifactSummary, ArtifactSyncTag};
use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use crate::local_store::LocalStore;
use std::collections::{HashMap, HashSet};

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
            ArtifactSummary::fetch_remote_components(&api_key, &self.api_endpoint).await
        } else {
            Err(eyre::eyre!("not authenticated"))
        };
        // Build remote version lookup.
        let remote_version_map = match &remote_result {
            Ok(summaries) => ArtifactSummary::remote_version_map(summaries.iter()),
            Err(_) => HashMap::new(),
        };
        // Build unified list.
        let mut entries: Vec<(String, ArtifactSyncTag)> = Vec::new();
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
            let local_ver = component.component().version().to_string();
            let remote_ver = remote_version_map.get(&id).copied();
            let tag = ArtifactSyncTag::resolve(Some(&local_ver), remote_ver);
            entries.push((ref_str, tag));
        }
        if let Ok(remote) = &remote_result {
            for comp in remote {
                let id = format!("{}:{}", comp.namespace, comp.name);
                if !local_refs.contains(&id) {
                    let ref_str =
                        format!("{}:{}@{}", comp.namespace, comp.name, comp.latest_version);
                    entries.push((
                        ref_str,
                        ArtifactSyncTag::resolve(None, Some(&comp.latest_version)),
                    ));
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
