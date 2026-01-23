use crate::auth::Auth;
use crate::cli_ext::environment::EnvironmentCliExt;
use crate::config::{API_URL, API_URL_STAGING, REGISTRY_URL, REGISTRY_URL_STAGING};
use crate::registry::{GetEnvironmentResponse, RegistryClient};
use asterai_runtime::component::Component;
use asterai_runtime::environment::{Environment, EnvironmentMetadata};
use asterai_runtime::resource::metadata::ResourceKind;
use eyre::{Context, OptionExt, bail};
use std::collections::HashMap;
use std::fs;
use std::str::FromStr;

#[derive(Debug)]
pub struct PullArgs {
    /// Environment reference (namespace:name or namespace:name@version).
    env_ref: String,
    api_endpoint: String,
    registry_endpoint: String,
    staging: bool,
    /// Whether to skip pulling components.
    manifest_only: bool,
}

impl PullArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut env_ref: Option<String> = None;
        let mut api_endpoint = API_URL.to_string();
        let mut registry_endpoint = REGISTRY_URL.to_string();
        let mut staging = false;
        let mut manifest_only = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--endpoint" | "-e" => {
                    api_endpoint = args.next().ok_or_eyre("missing value for endpoint flag")?;
                }
                "--registry" | "-r" => {
                    registry_endpoint =
                        args.next().ok_or_eyre("missing value for registry flag")?;
                }
                "--staging" | "-s" => {
                    staging = true;
                }
                "--manifest-only" | "-m" => {
                    manifest_only = true;
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
            "missing environment reference\n\nUsage: asterai env pull <namespace:name[@version]>\n\
             Example: asterai env pull myteam:my-env",
        )?;
        Ok(Self {
            env_ref,
            api_endpoint,
            registry_endpoint,
            staging,
            manifest_only,
        })
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        let api_key = Auth::read_stored_api_key()
            .ok_or_eyre("API key not found. Run 'asterai auth login' to authenticate.")?;
        // Parse environment reference.
        let (namespace, name, version) = parse_env_reference(&self.env_ref)?;
        println!(
            "pulling environment {}:{}{}...",
            namespace,
            name,
            version
                .as_ref()
                .map(|v| format!("@{}", v))
                .unwrap_or_default()
        );
        let (api_url, registry_url) = if self.staging {
            (API_URL_STAGING, REGISTRY_URL_STAGING)
        } else {
            (self.api_endpoint.as_str(), self.registry_endpoint.as_str())
        };
        // Fetch environment from API.
        let client = reqwest::Client::new();
        let url = if let Some(ver) = &version {
            format!("{}/v1/environment/{}/{}/{}", api_url, namespace, name, ver)
        } else {
            format!("{}/v1/environment/{}/{}", api_url, namespace, name)
        };
        let response = client
            .get(&url)
            .header("Authorization", api_key.trim())
            .send()
            .await
            .wrap_err("failed to fetch environment")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("environment '{}:{}' not found", namespace, name);
        }
        if response.status() == reqwest::StatusCode::FORBIDDEN {
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
        // Parse component refs into components map (namespace:name -> version).
        let mut components_map: HashMap<String, String> = HashMap::new();
        let mut component_list: Vec<Component> = Vec::new();

        for comp_ref in &env_data.components {
            let component = Component::from_str(comp_ref)
                .wrap_err_with(|| format!("failed to parse component: {}", comp_ref))?;
            let key = format!("{}:{}", component.namespace(), component.name());
            components_map.insert(key, component.version().to_string());
            component_list.push(component);
        }
        // Create local environment using new structure.
        let environment = Environment {
            metadata: EnvironmentMetadata {
                namespace: env_data.namespace.clone(),
                name: env_data.name.clone(),
                version: env_data.version.clone(),
            },
            components: components_map,
            vars: env_data.vars,
        };
        environment.write_to_disk()?;
        // Write metadata.
        let metadata_path = environment.local_metadata_file_path();
        let metadata = serde_json::json!({
            "kind": ResourceKind::Environment.to_string(),
            "pulled_from": format!("{}:{}@{}", env_data.namespace, env_data.name, env_data.version),
        });
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;
        println!("  saved to {}", environment.local_disk_dir().display());
        // Pull component WASMs unless manifest-only.
        if !self.manifest_only {
            println!("\npulling components...");
            let registry = RegistryClient::new(&client, api_url, registry_url);
            for component in &component_list {
                registry.pull_component(&api_key, component, false).await?;
            }
        }
        println!("\ndone");
        Ok(())
    }
}

/// Parse an environment reference like "namespace:name" or "namespace:name@version".
fn parse_env_reference(s: &str) -> eyre::Result<(String, String, Option<String>)> {
    // Check for version.
    let (id_part, version) = if let Some((id, ver)) = s.split_once('@') {
        (id, Some(ver.to_string()))
    } else {
        (s, None)
    };
    // Parse namespace:name or namespace/name.
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
        r#"Pull an environment from the registry.

Usage: asterai env pull <namespace:name[@version]> [options]

Arguments:
  <namespace:name[@version]>  Environment reference (pulls latest if no version specified)

Options:
  -m, --manifest-only     Only pull the environment manifest, not component WASMs
  -h, --help              Show this help message

Examples:
  asterai env pull myteam:my-env             # Pull latest version
  asterai env pull myteam:my-env@1.2.0       # Pull specific version
  asterai env pull myteam:my-env --staging   # Pull from staging
"#
    );
}
