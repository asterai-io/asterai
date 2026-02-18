use crate::auth::Auth;
use crate::command::auth::validate_api_key;
use crate::command::env::EnvArgs;
use crate::command::env::inspect::InspectData;
use crate::command::env::list::EnvListEntry;
use crate::command::env::pull::PullArgs;
use crate::command::env::push::PushArgs;
use crate::command::env::set_var::SetVarArgs;
use crate::config::{API_URL, REGISTRY_URL};
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

/// List environments.
pub async fn list_environments() -> eyre::Result<Vec<EnvListEntry>> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_list(api, registry);
    args.list_entries().await
}

/// Inspect an environment.
pub async fn inspect_environment(env_name: &str) -> eyre::Result<Option<InspectData>> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_inspect(env_name, api, registry);
    args.inspect_data()
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

fn endpoints() -> (String, String) {
    (API_URL.to_string(), REGISTRY_URL.to_string())
}
