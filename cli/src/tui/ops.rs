use crate::artifact::{ArtifactSummary, ArtifactSyncTag};
use crate::auth::Auth;
use crate::command::auth::validate_api_key;
use crate::command::env::EnvArgs;
use crate::command::env::inspect::InspectData;
use crate::command::env::list::{EnvListEntry, deduplicate_local_envs};
use crate::command::env::pull::PullArgs;
use crate::command::env::push::PushArgs;
use crate::command::env::set_var::SetVarArgs;
use crate::config::{API_URL, REGISTRY_URL};
use crate::local_store::LocalStore;
use crate::tui::app::{AgentConfig, resolve_state_dir};
use std::path::PathBuf;

/// Check if logged in. Returns username slug or None.
pub async fn check_auth() -> Option<String> {
    let api_key = Auth::read_stored_api_key()?;
    let (api, _) = endpoints();
    validate_api_key(&api_key, &api).await.ok()
}

/// Log in with an API key. Returns username slug.
pub async fn login(api_key: &str) -> eyre::Result<String> {
    let (api, _) = endpoints();
    let slug = validate_api_key(api_key, &api).await?;
    Auth::store_api_key(api_key)?;
    Auth::store_user_namespace(&slug)?;
    Ok(slug)
}

/// List environments (local + remote sync check).
pub async fn list_environments() -> eyre::Result<Vec<EnvListEntry>> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_list(api, registry);
    args.list_entries().await
}

/// List only local environments (no network call). Fast.
pub fn list_local_environments() -> Vec<EnvListEntry> {
    deduplicate_local_envs(LocalStore::list_environments())
        .into_iter()
        .map(|env| EnvListEntry {
            namespace: env.namespace().to_string(),
            name: env.name().to_string(),
            version: Some(env.version().to_string()),
            remote_version: None,
            component_count: env.components.len(),
            sync_tag: ArtifactSyncTag::Unpushed,
        })
        .collect()
}

/// Inspect an environment (async wrapper).
pub async fn inspect_environment(env_name: &str) -> eyre::Result<Option<InspectData>> {
    Ok(inspect_environment_sync(env_name))
}

/// Inspect an environment (synchronous, filesystem only).
pub fn inspect_environment_sync(env_name: &str) -> Option<InspectData> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_inspect(env_name, api, registry);
    args.inspect_data().ok().flatten()
}

/// Call the converse function on an agent environment.
pub async fn call_converse(message: &str, agent: &AgentConfig) -> eyre::Result<Option<String>> {
    let (api, registry) = endpoints();
    let state_dir = resolve_state_dir(&agent.env_name);
    let mut allow_dirs: Vec<PathBuf> = vec![state_dir];
    for dir in &agent.allowed_dirs {
        allow_dirs.push(PathBuf::from(dir));
    }
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    let args = EnvArgs::for_call(
        &agent.env_name,
        "asterbot:agent",
        "agent/converse",
        vec![format!("\"{escaped}\"")],
        allow_dirs,
        api,
        registry,
    );
    args.call_returning().await
}

/// Init an environment.
pub fn env_init(env_name: &str) -> eyre::Result<()> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_init(env_name, api, registry);
    args.init()
}

/// Add a component to an environment.
pub async fn add_component(env_name: &str, component: &str) -> eyre::Result<()> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_add_component(env_name, component, api, registry)?;
    args.add_component().await
}

/// Remove a component from an environment.
pub async fn remove_component(env_name: &str, component: &str) -> eyre::Result<()> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_remove_component(env_name, component, api, registry)?;
    args.remove_component().await
}

/// Set a variable on an environment.
pub fn set_var(env_name: &str, key: &str, value: &str) -> eyre::Result<()> {
    let args_vec = vec![
        env_name.to_string(),
        "--var".to_string(),
        format!("{key}={value}"),
    ];
    let set_var_args = SetVarArgs::parse(args_vec.into_iter())?;
    set_var_args.execute()
}

/// Push an environment.
pub async fn push_env(env_name: &str) -> eyre::Result<()> {
    let (api, _) = endpoints();
    let args = PushArgs::parse(vec![env_name.to_string()].into_iter())?;
    args.execute(&api).await
}

/// Pull an environment.
pub async fn pull_env(env_name: &str) -> eyre::Result<()> {
    let (api, registry) = endpoints();
    let args = PullArgs::parse(vec![env_name.to_string()].into_iter())?;
    args.execute(&api, &registry).await
}

/// Fetch all components from the remote registry.
/// Returns (namespace, name, latest_version) tuples.
pub async fn list_remote_components() -> eyre::Result<Vec<(String, String, String)>> {
    let api_key = Auth::read_stored_api_key().ok_or_else(|| eyre::eyre!("not authenticated"))?;
    let (api, _) = endpoints();
    let components = ArtifactSummary::fetch_remote_components(&api_key, &api).await?;
    Ok(components
        .into_iter()
        .map(|c| (c.namespace, c.name, c.latest_version))
        .collect())
}

/// Fetch the latest CLI version from crates.io.
pub async fn fetch_latest_cli_version() -> Option<String> {
    #[derive(serde::Deserialize)]
    struct CrateResponse {
        #[serde(rename = "crate")]
        krate: CrateInfo,
    }
    #[derive(serde::Deserialize)]
    struct CrateInfo {
        max_version: String,
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let resp = client
        .get("https://crates.io/api/v1/crates/asterai")
        .header("User-Agent", "asterai-cli")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: CrateResponse = resp.json().await.ok()?;
    Some(data.krate.max_version)
}

/// Delete a local environment (all versions) by namespace and name.
pub fn delete_local_env(namespace: &str, name: &str) -> eyre::Result<usize> {
    let ns_dir = crate::config::ARTIFACTS_DIR.join(namespace);
    if !ns_dir.exists() {
        return Ok(0);
    }
    let prefix = format!("{name}@");
    let mut removed = 0;
    for entry in std::fs::read_dir(&ns_dir)? {
        let entry = entry?;
        if let Some(fname) = entry.file_name().to_str()
            && fname.starts_with(&prefix)
        {
            // Only delete if it contains env.toml (is an environment, not a component).
            if entry.path().join("env.toml").exists() {
                std::fs::remove_dir_all(entry.path())?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

fn endpoints() -> (String, String) {
    (API_URL.to_string(), REGISTRY_URL.to_string())
}
