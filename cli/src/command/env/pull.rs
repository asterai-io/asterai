use crate::auth::Auth;
use crate::cli_ext::environment::EnvironmentCliExt;
use crate::config::{API_URL, API_URL_STAGING, BIN_DIR, REGISTRY_URL, REGISTRY_URL_STAGING};
use asterai_runtime::component::Component;
use asterai_runtime::environment::{Environment, EnvironmentMetadata};
use asterai_runtime::resource::metadata::ResourceKind;
use eyre::{Context, OptionExt, bail};
use serde::Deserialize;
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

/// Response from getting an environment.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetEnvironmentResponse {
    namespace: String,
    name: String,
    version: String,
    components: Vec<String>,
    vars: HashMap<String, String>,
}

/// Token response from the registry.
#[derive(Deserialize)]
struct TokenResponse {
    token: String,
    #[allow(dead_code)]
    expires_in: u64,
    #[allow(dead_code)]
    issued_at: i64,
}

/// OCI manifest structure.
#[derive(Deserialize)]
struct OciManifest {
    #[serde(rename = "schemaVersion")]
    #[allow(dead_code)]
    schema_version: u32,
    #[allow(dead_code)]
    layers: Vec<OciDescriptor>,
}

#[derive(Deserialize)]
struct OciDescriptor {
    #[serde(rename = "mediaType")]
    #[allow(dead_code)]
    media_type: String,
    digest: String,
    #[allow(dead_code)]
    size: u64,
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
            for component in &component_list {
                self.pull_component(&client, &api_key, api_url, registry_url, component)
                    .await?;
            }
        }
        println!("\ndone");
        Ok(())
    }

    async fn pull_component(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        api_url: &str,
        registry_url: &str,
        component: &Component,
    ) -> eyre::Result<()> {
        let namespace = component.namespace();
        let name = component.name();
        let version = component.version().to_string();
        let repo_name = format!("{}/{}", namespace, name);
        println!("  pulling {}@{}...", repo_name, version);
        // Get registry token.
        let token = self
            .get_registry_token(client, api_key, api_url, &repo_name)
            .await?;
        // Fetch manifest.
        let manifest = self
            .fetch_manifest(client, registry_url, &repo_name, &version, &token)
            .await?;
        // Create output directory.
        let output_dir = BIN_DIR
            .join("resources")
            .join(namespace)
            .join(format!("{}@{}", name, version));
        fs::create_dir_all(&output_dir)?;
        // Download layers.
        for (i, layer) in manifest.layers.iter().enumerate() {
            let blob_url = format!("{}/v2/{}/blobs/{}", registry_url, repo_name, layer.digest);
            let blob_response = client
                .get(&blob_url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
                .wrap_err("failed to fetch blob")?;
            if !blob_response.status().is_success() {
                let status = blob_response.status();
                let error_text = blob_response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown error".to_string());
                bail!("failed to fetch blob ({}): {}", status, error_text);
            }
            let blob_bytes = blob_response.bytes().await?;
            let filename = if i == 0 {
                "component.wasm"
            } else {
                "package.wasm"
            };
            let file_path = output_dir.join(filename);
            fs::write(&file_path, &blob_bytes)?;
        }
        // Write component metadata.
        let metadata = serde_json::json!({
            "kind": ResourceKind::Component.to_string(),
            "pulled_from": format!("{}@{}", repo_name, version),
        });
        let metadata_path = output_dir.join("metadata.json");
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;
        println!("    saved to {}", output_dir.display());
        Ok(())
    }

    async fn get_registry_token(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        api_url: &str,
        repo_name: &str,
    ) -> eyre::Result<String> {
        let scope = format!("repository:{}:pull", repo_name);
        let token_url = format!("{}/v1/registry/token?scope={}", api_url, scope);
        let response = client
            .get(&token_url)
            .header("Authorization", format!("Bearer {}", api_key.trim()))
            .send()
            .await
            .wrap_err("failed to get registry token")?;
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to get registry token ({}): {}", status, error_text);
        }
        let token_response: TokenResponse = response
            .json()
            .await
            .wrap_err("failed to parse token response")?;
        Ok(token_response.token)
    }

    async fn fetch_manifest(
        &self,
        client: &reqwest::Client,
        registry_url: &str,
        repo_name: &str,
        tag: &str,
        token: &str,
    ) -> eyre::Result<OciManifest> {
        let manifest_url = format!("{}/v2/{}/manifests/{}", registry_url, repo_name, tag);
        let response = client
            .get(&manifest_url)
            .header("Authorization", format!("Bearer {}", token))
            .header(
                "Accept",
                "application/vnd.oci.image.manifest.v1+json, \
                application/vnd.docker.distribution.manifest.v2+json",
            )
            .send()
            .await
            .wrap_err("failed to fetch manifest")?;
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to fetch manifest ({}): {}", status, error_text);
        }
        let manifest: OciManifest = response.json().await.wrap_err("failed to parse manifest")?;
        Ok(manifest)
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
