use crate::artifact::{ArtifactSummary, ArtifactSyncTag};
use crate::auth::Auth;
use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use asterai_runtime::environment::Environment;
use semver::Version;
use std::collections::{HashMap, HashSet};

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
        let local_envs = deduplicate_local_envs(LocalStore::list_environments());
        let local_refs: HashSet<String> = local_envs
            .iter()
            .map(|e| format!("{}:{}", e.namespace(), e.name()))
            .collect();
        let remote_result = if let Some(api_key) = Auth::read_stored_api_key() {
            ArtifactSummary::fetch_remote_environments(&api_key, &self.api_endpoint).await
        } else {
            Err(eyre::eyre!("not authenticated"))
        };
        let remote_version_map = match &remote_result {
            Ok(summaries) => ArtifactSummary::remote_version_map(summaries.iter()),
            Err(_) => HashMap::new(),
        };
        let mut entries = Vec::new();
        for env in &local_envs {
            let id = format!("{}:{}", env.namespace(), env.name());
            let remote_ver = remote_version_map.get(&id).copied();
            let tag = ArtifactSyncTag::resolve(Some(env.version()), remote_ver);
            entries.push(EnvListEntry {
                namespace: env.namespace().to_string(),
                name: env.name().to_string(),
                version: Some(env.version().to_string()),
                remote_version: remote_ver.map(|v| v.to_string()),
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
                        sync_tag: ArtifactSyncTag::resolve(None, Some(&env.latest_version)),
                    });
                }
            }
        }
        Ok(entries)
    }
}

/// Deduplicate environments by namespace:name, keeping the highest semver version.
pub fn deduplicate_local_envs(envs: Vec<Environment>) -> Vec<Environment> {
    let mut map: HashMap<String, Environment> = HashMap::new();
    for env in envs {
        let id = format!("{}:{}", env.namespace(), env.name());
        let is_newer = match map.get(&id) {
            None => true,
            Some(prev) => {
                let cur = Version::parse(env.version()).unwrap_or(Version::new(0, 0, 0));
                let old = Version::parse(prev.version()).unwrap_or(Version::new(0, 0, 0));
                cur > old
            }
        };
        if is_newer {
            map.insert(id, env);
        }
    }
    map.into_values().collect()
}
