use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use crate::config::BIN_DIR;
use asterai_runtime::resource::Resource;
use asterai_runtime::resource::metadata::ResourceKind;
use eyre::{Context, OptionExt, bail};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

const BASE_API_URL: &str = "https://api.asterai.io";
const BASE_API_URL_STAGING: &str = "https://staging.api.asterai.io";
const BASE_REGISTRY_URL: &str = "https://registry.asterai.io";
const BASE_REGISTRY_URL_STAGING: &str = "https://staging.registry.asterai.io";

#[derive(Debug)]
pub(super) struct PullArgs {
    /// Component resource reference.
    /// Accepts both WIT-style (namespace:name@version) and OCI-style (namespace/name@version).
    component: Resource,
    /// Custom API endpoint for token retrieval.
    api_endpoint: String,
    /// Custom registry endpoint.
    registry_endpoint: String,
    /// Use staging environment.
    staging: bool,
    /// Output directory (defaults to local resource storage).
    output: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    token: String,
    #[allow(dead_code)]
    expires_in: u64,
    #[allow(dead_code)]
    issued_at: i64,
}

#[derive(Deserialize)]
struct OciManifest {
    #[serde(rename = "schemaVersion")]
    #[allow(dead_code)]
    schema_version: u32,
    #[serde(rename = "mediaType")]
    #[allow(dead_code)]
    media_type: Option<String>,
    config: OciDescriptor,
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
        let mut component: Option<Resource> = None;
        let mut api_endpoint = BASE_API_URL.to_string();
        let mut registry_endpoint = BASE_REGISTRY_URL.to_string();
        let mut did_specify_registry = false;
        let mut staging = false;
        let mut output: Option<String> = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-r" | "--registry" => {
                    registry_endpoint =
                        args.next().ok_or_eyre("missing value for registry flag")?;
                    did_specify_registry = true;
                }
                "-e" | "--endpoint" => {
                    api_endpoint = args.next().ok_or_eyre("missing value for endpoint flag")?;
                    if !did_specify_registry {
                        registry_endpoint = BASE_REGISTRY_URL_STAGING.to_string();
                    }
                }
                "-s" | "--staging" => {
                    staging = true;
                }
                "-o" | "--output" => {
                    output = Some(args.next().ok_or_eyre("missing value for output flag")?);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if component.is_some() {
                        bail!("unexpected argument: {}", other);
                    }
                    component = Some(Resource::from_str(other).wrap_err(
                        "invalid component reference \
                         (expected namespace:name@version or namespace/name@version)",
                    )?);
                }
            }
        }
        let component = component.ok_or_eyre(
            "missing component reference \
            (e.g., namespace:component@version or namespace/component@version)",
        )?;
        Ok(Self {
            component,
            api_endpoint,
            registry_endpoint,
            staging,
            output,
        })
    }

    async fn execute(&self) -> eyre::Result<()> {
        let namespace = self.component.namespace();
        let name = self.component.name();
        let version = self.component.version().to_string();
        let repo_name = format!("{}/{}", namespace, name);
        let tag = &version;
        println!("pulling {}@{}", repo_name, tag);
        let (api_url, registry_url) = if self.staging {
            (BASE_API_URL_STAGING, BASE_REGISTRY_URL_STAGING)
        } else {
            (self.api_endpoint.as_str(), self.registry_endpoint.as_str())
        };
        let token = self.get_registry_token(api_url, &repo_name).await?;
        let client = reqwest::Client::new();
        let manifest = self
            .fetch_manifest(&client, registry_url, &repo_name, tag, &token)
            .await?;
        let output_dir = self.determine_output_dir(namespace, name, &version)?;
        self.download_layers(
            &client,
            registry_url,
            &repo_name,
            &token,
            &manifest.layers,
            &output_dir,
        )
        .await?;
        self.write_metadata(&output_dir, &repo_name, tag)?;
        println!("pulled component saved to {}", output_dir.display());
        Ok(())
    }

    async fn fetch_manifest(
        &self,
        client: &reqwest::Client,
        registry_url: &str,
        repo_name: &str,
        tag: &str,
        token: &str,
    ) -> eyre::Result<OciManifest> {
        println!("fetching manifest...");
        let manifest_url = format!("{}/v2/{}/manifests/{}", registry_url, repo_name, tag);
        let manifest_response = client
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
        if !manifest_response.status().is_success() {
            let status = manifest_response.status();
            let error_text = manifest_response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to fetch manifest ({}): {}", status, error_text);
        }
        let manifest: OciManifest = manifest_response
            .json()
            .await
            .wrap_err("failed to parse manifest")?;
        Ok(manifest)
    }

    fn determine_output_dir(
        &self,
        namespace: &str,
        name: &str,
        version: &str,
    ) -> eyre::Result<PathBuf> {
        let output_dir = match &self.output {
            Some(dir) => PathBuf::from(dir),
            None => BIN_DIR
                .join("resources")
                .join(namespace)
                .join(format!("{}@{}", name, version)),
        };
        fs::create_dir_all(&output_dir).wrap_err("failed to create output directory")?;
        Ok(output_dir)
    }

    async fn download_layers(
        &self,
        client: &reqwest::Client,
        registry_url: &str,
        repo_name: &str,
        token: &str,
        layers: &[OciDescriptor],
        output_dir: &PathBuf,
    ) -> eyre::Result<()> {
        for (i, layer) in layers.iter().enumerate() {
            println!("Downloading layer {} ({})...", i + 1, &layer.digest[..19]);
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
            let blob_bytes = blob_response
                .bytes()
                .await
                .wrap_err("failed to read blob bytes")?;
            let filename = if i == 0 {
                "component.wasm"
            } else {
                "package.wasm"
            };
            let file_path = output_dir.join(filename);
            fs::write(&file_path, &blob_bytes)
                .wrap_err_with(|| format!("failed to write {}", filename))?;
            println!("  Saved to {}", file_path.display());
        }
        Ok(())
    }

    fn write_metadata(&self, output_dir: &PathBuf, repo_name: &str, tag: &str) -> eyre::Result<()> {
        let metadata = serde_json::json!({
            "kind": ResourceKind::Component.to_string(),
            "pulled_from": format!("{}@{}", repo_name, tag),
        });
        let metadata_path = output_dir.join("metadata.json");
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)
            .wrap_err("failed to write metadata")?;
        Ok(())
    }

    async fn get_registry_token(&self, api_url: &str, repo_name: &str) -> eyre::Result<String> {
        let client = reqwest::Client::new();
        let scope = format!("repository:{}:pull", repo_name);
        let token_url = format!("{}/v1/registry/token?scope={}", api_url, scope);
        let mut request = client.get(&token_url);
        // Add API key if available (required for private repos, optional for public).
        if let Some(api_key) = Auth::read_stored_api_key() {
            request = request.header("Authorization", format!("Bearer {}", api_key.trim()));
        }
        let response = request
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
}

impl ComponentArgs {
    pub async fn pull(&self) -> eyre::Result<()> {
        let args = self.pull_args.as_ref().ok_or_eyre("no pull args")?;
        args.execute().await
    }
}
