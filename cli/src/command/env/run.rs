use crate::auth::Auth;
use crate::local_store::LocalStore;
use crate::registry::{GetEnvironmentResponse, RegistryClient};
use crate::runtime::build_runtime;
use asterai_runtime::component::Component;
use asterai_runtime::environment::{Environment, EnvironmentMetadata};
use asterai_runtime::resource::metadata::ResourceKind;
use eyre::{Context, OptionExt, bail};
use reqwest::StatusCode;
use std::collections::HashMap;
use std::fs;
use std::str::FromStr;

#[derive(Debug)]
pub(super) struct RunArgs {
    /// Environment reference (namespace:name or namespace:name@version).
    // TODO also support just `name` and default to user's personal namespace
    env_ref: String,
    /// If true, don't pull from registry - use cached version only.
    no_pull: bool,
}

impl RunArgs {
    pub fn parse(args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut env_ref: Option<String> = None;
        let mut no_pull = false;
        for arg in args {
            match arg.as_str() {
                "--no-pull" => {
                    no_pull = true;
                }
                "--help" | "-h" | "help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if env_ref.is_some() {
                        bail!("unexpected argument: {}", other);
                    }
                    env_ref = Some(other.to_string());
                }
            }
        }
        let env_ref = env_ref.ok_or_eyre(
            "missing environment reference\n\nUsage: asterai env run <namespace:name[@version]>\n\
             Example: asterai env run myteam:my-env",
        )?;
        Ok(Self { env_ref, no_pull })
    }

    pub async fn execute(&self, api_endpoint: &str, registry_endpoint: &str) -> eyre::Result<()> {
        let (namespace, name, version) = parse_env_reference(&self.env_ref)?;
        // Try to find environment locally first.
        let local_env = self.find_local_environment(&namespace, &name, version.as_deref());
        let environment = match local_env {
            Some(env) => {
                println!(
                    "running environment {}:{}@{} (cached)",
                    env.namespace(),
                    env.name(),
                    env.version()
                );
                env
            }
            None => {
                if self.no_pull {
                    bail!(
                        "environment '{}:{}{}' not found locally (use without --no-pull to fetch from registry)",
                        namespace,
                        name,
                        version
                            .as_ref()
                            .map(|v| format!("@{}", v))
                            .unwrap_or_default()
                    );
                }
                // Pull from registry.
                self.pull_environment(
                    &namespace,
                    &name,
                    version.as_deref(),
                    api_endpoint,
                    registry_endpoint,
                )
                .await?
            }
        };
        // Run the environment.
        let mut runtime = build_runtime(environment).await?;
        runtime.run().await?;
        Ok(())
    }

    fn find_local_environment(
        &self,
        namespace: &str,
        name: &str,
        version: Option<&str>,
    ) -> Option<Environment> {
        let local_envs = LocalStore::list_environments();
        if let Some(ver) = version {
            // Look for specific version.
            local_envs.into_iter().find(|env| {
                env.namespace() == namespace && env.name() == name && env.version() == ver
            })
        } else {
            // Find latest local version for this namespace:name.
            local_envs
                .into_iter()
                .filter(|env| env.namespace() == namespace && env.name() == name)
                .max_by(|a, b| {
                    // Compare versions using semver if possible.
                    let ver_a = semver::Version::parse(a.version()).ok();
                    let ver_b = semver::Version::parse(b.version()).ok();
                    match (ver_a, ver_b) {
                        (Some(va), Some(vb)) => va.cmp(&vb),
                        _ => a.version().cmp(b.version()),
                    }
                })
        }
    }

    async fn pull_environment(
        &self,
        namespace: &str,
        name: &str,
        version: Option<&str>,
        api_endpoint: &str,
        registry_endpoint: &str,
    ) -> eyre::Result<Environment> {
        let api_key = Auth::read_stored_api_key()
            .ok_or_eyre("API key not found. Run 'asterai auth login' to authenticate.")?;
        println!(
            "pulling environment {}:{}{}...",
            namespace,
            name,
            version.map(|v| format!("@{}", v)).unwrap_or_default()
        );
        // Fetch environment from API.
        let client = reqwest::Client::new();
        let url = match version {
            Some(ver) => format!(
                "{}/v1/environment/{}/{}/{}",
                api_endpoint, namespace, name, ver
            ),
            None => format!("{}/v1/environment/{}/{}", api_endpoint, namespace, name),
        };
        let response = client
            .get(&url)
            .header("Authorization", api_key.trim())
            .send()
            .await
            .wrap_err("failed to fetch environment")?;
        if response.status() == StatusCode::NOT_FOUND {
            bail!("environment '{}:{}' not found in registry", namespace, name);
        }
        if response.status() == StatusCode::FORBIDDEN {
            bail!(
                "forbidden: you don't have access to environment '{}:{}'",
                namespace,
                name
            );
        }
        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to fetch environment: {}", error_text);
        }
        let env_data: GetEnvironmentResponse = response
            .json()
            .await
            .wrap_err("failed to parse environment response")?;
        println!("  version: {}", env_data.version);
        println!("  components: {}", env_data.components.len());
        // Parse component refs into components map.
        let mut components_map: HashMap<String, String> = HashMap::new();
        let mut component_list: Vec<Component> = Vec::new();
        for comp_ref in &env_data.components {
            let component = Component::from_str(comp_ref)
                .wrap_err_with(|| format!("failed to parse component: {}", comp_ref))?;
            let key = format!("{}:{}", component.namespace(), component.name());
            components_map.insert(key, component.version().to_string());
            component_list.push(component);
        }
        // Create local environment.
        let environment = Environment {
            metadata: EnvironmentMetadata {
                namespace: env_data.namespace.clone(),
                name: env_data.name.clone(),
                version: env_data.version.clone(),
            },
            components: components_map,
            vars: env_data.vars,
        };
        LocalStore::write_environment(&environment)?;
        // Write additional metadata (pulled_from).
        let env_dir = LocalStore::environment_dir(&environment);
        let metadata_path = env_dir.join("metadata.json");
        let metadata = serde_json::json!({
            "kind": ResourceKind::Environment.to_string(),
            "pulled_from": format!("{}:{}@{}", env_data.namespace, env_data.name, env_data.version),
        });
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;
        println!("  saved to {}", env_dir.display());
        // Pull component WASMs using shared registry client.
        println!("\npulling components...");
        let registry = RegistryClient::new(&client, api_endpoint, registry_endpoint);
        for component in &component_list {
            registry.pull_component(&api_key, component, false).await?;
        }
        Ok(environment)
    }
}

/// Parse an environment reference like "namespace:name" or "namespace:name@version".
fn parse_env_reference(s: &str) -> eyre::Result<(String, String, Option<String>)> {
    let (id_part, version) = match s.split_once('@') {
        Some((id, ver)) => (id, Some(ver.to_string())),
        None => (s, None),
    };
    let (namespace, name) = id_part
        .split_once(':')
        .or_else(|| id_part.split_once('/'))
        .ok_or_else(|| {
            eyre::eyre!(
                "invalid environment reference '{}': use namespace:name or namespace:name@version",
                s
            )
        })?;
    Ok((namespace.to_string(), name.to_string(), version))
}

fn print_help() {
    println!(
        r#"Run an environment locally.

Usage: asterai env run <namespace:name[@version]> [options]

Arguments:
  <namespace:name[@version]>  Environment reference (runs latest if no version specified)

Options:
  --no-pull             Don't pull from registry, use cached version only
  -h, --help            Show this help message

Examples:
  asterai env run myteam:my-env             # Pull (if needed) and run latest
  asterai env run myteam:my-env@1.2.0       # Pull (if needed) and run specific version
  asterai env run myteam:my-env --no-pull   # Run cached version only
"#
    );
}
