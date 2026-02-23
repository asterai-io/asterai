use crate::artifact::ArtifactSyncTag;
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

/// List environments (local + remote sync check).
pub async fn list_environments() -> eyre::Result<Vec<EnvListEntry>> {
    let (api, registry) = endpoints();
    let args = EnvArgs::for_list(api, registry);
    args.list_entries().await
}

/// List only local environments (no network call). Fast.
pub fn list_local_environments() -> Vec<EnvListEntry> {
    use crate::local_store::LocalStore;
    use std::collections::HashMap;
    let all_local = LocalStore::list_environments();
    let mut local_map: HashMap<String, asterai_runtime::environment::Environment> = HashMap::new();
    for env in all_local {
        let id = format!("{}:{}", env.namespace(), env.name());
        let dominated = match local_map.get(&id) {
            None => true,
            Some(prev) => {
                let cur =
                    semver::Version::parse(env.version()).unwrap_or(semver::Version::new(0, 0, 0));
                let old =
                    semver::Version::parse(prev.version()).unwrap_or(semver::Version::new(0, 0, 0));
                cur > old
            }
        };
        if dominated {
            local_map.insert(id, env);
        }
    }
    local_map
        .into_values()
        .map(|env| EnvListEntry {
            namespace: env.namespace().to_string(),
            name: env.name().to_string(),
            version: Some(env.version().to_string()),
            remote_version: None,
            component_count: env.components.len(),
            sync_tag: ArtifactSyncTag::Unpushed, // Placeholder until remote sync check.
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

/// Call the converse function via a running process's HTTP API.
pub async fn call_converse_via_process(
    message: &str,
    namespace: &str,
    env_name: &str,
    port: u16,
) -> eyre::Result<Option<String>> {
    let url = format!("http://127.0.0.1:{port}/v1/environment/{namespace}/{env_name}/call");
    let body = serde_json::json!({
        "component": "asterbot:agent",
        "function": "agent/converse",
        "args": [message],
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        eyre::bail!("process returned {status}: {text}");
    }
    #[derive(serde::Deserialize)]
    struct CallResponse {
        output: Option<serde_json::Value>,
    }
    let data: CallResponse = resp.json().await?;
    Ok(data.output.and_then(|v| match v {
        serde_json::Value::String(s) => Some(s),
        other => Some(other.to_string()),
    }))
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
    let components =
        crate::command::component::list::fetch_remote_components(&api_key, &api).await?;
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
        if let Some(fname) = entry.file_name().to_str() {
            if fname.starts_with(&prefix) {
                // Only delete if it contains env.toml (is an environment, not a component).
                if entry.path().join("env.toml").exists() {
                    std::fs::remove_dir_all(entry.path())?;
                    removed += 1;
                }
            }
        }
    }
    Ok(removed)
}

/// Scan the system for running `asterai env run` processes.
pub fn scan_running_agents() -> Vec<crate::tui::app::RunningAgent> {
    #[cfg(windows)]
    {
        scan_running_agents_windows()
    }
    #[cfg(unix)]
    {
        scan_running_agents_unix()
    }
}

#[cfg(windows)]
fn scan_running_agents_windows() -> Vec<crate::tui::app::RunningAgent> {
    let ps_cmd = r#"Get-CimInstance Win32_Process -Filter "Name='asterai.exe'" | Select-Object ProcessId,CommandLine | ConvertTo-Json -Compress"#;
    let output = match std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_cmd])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    #[derive(serde::Deserialize)]
    struct ProcInfo {
        #[serde(alias = "ProcessId")]
        process_id: u32,
        #[serde(alias = "CommandLine")]
        command_line: Option<String>,
    }
    let infos: Vec<ProcInfo> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).unwrap_or_default()
    } else {
        match serde_json::from_str::<ProcInfo>(trimmed) {
            Ok(info) => vec![info],
            Err(_) => Vec::new(),
        }
    };
    infos
        .into_iter()
        .filter_map(|info| {
            let cmd = info.command_line.as_deref()?;
            parse_env_run_command(cmd, info.process_id)
        })
        .collect()
}

#[cfg(unix)]
fn scan_running_agents_unix() -> Vec<crate::tui::app::RunningAgent> {
    let output = match std::process::Command::new("ps")
        .args(["-eo", "pid,args"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.contains("asterai") || !line.contains("env") || !line.contains("run") {
                return None;
            }
            let mut parts = line.splitn(2, char::is_whitespace);
            let pid: u32 = parts.next()?.trim().parse().ok()?;
            let cmd = parts.next()?.trim();
            parse_env_run_command(cmd, pid)
        })
        .collect()
}

/// Parse an `asterai env run <name> [--port PORT]` command line.
fn parse_env_run_command(cmdline: &str, pid: u32) -> Option<crate::tui::app::RunningAgent> {
    let args: Vec<&str> = cmdline.split_whitespace().collect();
    let env_idx = args.iter().position(|a| *a == "env")?;
    if args.get(env_idx + 1).copied() != Some("run") {
        return None;
    }
    let name_arg = args.get(env_idx + 2)?;
    if name_arg.starts_with('-') {
        return None;
    }
    // Strip @version suffix for matching.
    let base_name = name_arg.split('@').next().unwrap_or(name_arg).to_string();
    // Extract port from -p/--port.
    let mut port: u16 = 8080;
    for (i, arg) in args.iter().enumerate() {
        if *arg == "-p" || *arg == "--port" {
            if let Some(val) = args.get(i + 1) {
                if let Ok(p) = val.parse::<u16>() {
                    port = p;
                }
            }
        }
    }
    Some(crate::tui::app::RunningAgent {
        name: base_name,
        port,
        pid,
    })
}

/// Kill a process by PID.
pub fn kill_process(pid: u32) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .map_err(|e| eyre::eyre!("failed to run taskkill: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eyre::bail!("taskkill failed: {}", stderr.trim());
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let output = std::process::Command::new("kill")
            .args([&pid.to_string()])
            .output()
            .map_err(|e| eyre::eyre!("failed to run kill: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eyre::bail!("kill failed: {}", stderr.trim());
        }
        Ok(())
    }
}

/// Find the next available port starting from 8080.
pub fn next_available_port(running: &[crate::tui::app::RunningAgent]) -> u16 {
    let used: std::collections::HashSet<u16> = running.iter().map(|r| r.port).collect();
    let mut port: u16 = 8080;
    while used.contains(&port) {
        port = port.saturating_add(1);
        if port == 0 {
            break;
        }
    }
    port
}

/// Start an agent as a detached background process.
/// Returns the child PID on success.
pub fn start_agent_process(
    env_name: &str,
    port: u16,
    allowed_dirs: &[String],
) -> eyre::Result<u32> {
    let exe = std::env::current_exe()
        .map_err(|e| eyre::eyre!("failed to get current executable path: {e}"))?;
    let state_dir = resolve_state_dir(env_name);
    let _ = std::fs::create_dir_all(&state_dir);
    let mut args = vec![
        "env".to_string(),
        "run".to_string(),
        env_name.to_string(),
        "--no-pull".to_string(),
        "-p".to_string(),
        port.to_string(),
        "--allow-dir".to_string(),
        state_dir.to_string_lossy().to_string(),
    ];
    for dir in allowed_dirs {
        args.push("--allow-dir".to_string());
        args.push(dir.clone());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;
        let child = std::process::Command::new(&exe)
            .args(&args)
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| eyre::eyre!("failed to spawn agent process: {e}"))?;
        Ok(child.id())
    }
    #[cfg(unix)]
    {
        let child = std::process::Command::new(&exe)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| eyre::eyre!("failed to spawn agent process: {e}"))?;
        Ok(child.id())
    }
}

/// Get allowed_dirs for an agent from its environment config.
pub async fn get_agent_allowed_dirs(env_name: &str) -> Vec<String> {
    let data = match inspect_environment(env_name).await {
        Ok(Some(d)) => d,
        _ => return Vec::new(),
    };
    data.var_values
        .get("ASTERBOT_ALLOWED_DIRS")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn endpoints() -> (String, String) {
    (API_URL.to_string(), REGISTRY_URL.to_string())
}
