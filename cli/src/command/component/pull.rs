use crate::command::component::ComponentArgs;
use crate::config::{API_URL, API_URL_STAGING, ARTIFACTS_DIR, REGISTRY_URL, REGISTRY_URL_STAGING};
use crate::registry::RegistryClient;
use asterai_runtime::resource::Resource;
use asterai_runtime::resource::metadata::ResourceKind;
use eyre::{Context, OptionExt, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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

impl PullArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut component: Option<Resource> = None;
        let mut api_endpoint = API_URL.to_string();
        let mut registry_endpoint = REGISTRY_URL.to_string();
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
                        registry_endpoint = REGISTRY_URL_STAGING.to_string();
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
        let (api_url, registry_url) = match self.staging {
            true => (API_URL_STAGING, REGISTRY_URL_STAGING),
            false => (self.api_endpoint.as_str(), self.registry_endpoint.as_str()),
        };
        let client = reqwest::Client::new();
        let registry = RegistryClient::new(&client, api_url, registry_url);
        let token = registry.get_token_optional_auth(&repo_name).await?;
        println!("fetching manifest...");
        let manifest = registry.fetch_manifest(&repo_name, tag, &token).await?;
        let output_dir = self.determine_output_dir(namespace, name, &version)?;
        // Download layers.
        for (i, layer) in manifest.layers.iter().enumerate() {
            println!("downloading layer {} ({})...", i + 1, &layer.digest[..19]);
            let blob_bytes = registry
                .download_blob(&repo_name, &layer.digest, &token)
                .await?;
            let filename = match i {
                0 => "component.wasm",
                _ => "package.wasm",
            };
            let file_path = output_dir.join(filename);
            fs::write(&file_path, &blob_bytes)
                .wrap_err_with(|| format!("failed to write {}", filename))?;
            println!("  saved to {}", file_path.display());
        }
        self.write_metadata(&output_dir, &repo_name, tag)?;
        println!("pulled component saved to {}", output_dir.display());
        Ok(())
    }

    fn determine_output_dir(
        &self,
        namespace: &str,
        name: &str,
        version: &str,
    ) -> eyre::Result<PathBuf> {
        let output_dir = match &self.output {
            Some(dir) => PathBuf::from(dir),
            None => ARTIFACTS_DIR
                .join(namespace)
                .join(format!("{}@{}", name, version)),
        };
        fs::create_dir_all(&output_dir).wrap_err("failed to create output directory")?;
        Ok(output_dir)
    }

    fn write_metadata(&self, output_dir: &Path, repo_name: &str, tag: &str) -> eyre::Result<()> {
        let metadata = serde_json::json!({
            "kind": ResourceKind::Component.to_string(),
            "pulled_from": format!("{}@{}", repo_name, tag),
        });
        let metadata_path = output_dir.join("metadata.json");
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)
            .wrap_err("failed to write metadata")?;
        Ok(())
    }
}

impl ComponentArgs {
    pub async fn pull(&self) -> eyre::Result<()> {
        let args = self.pull_args.as_ref().ok_or_eyre("no pull args")?;
        args.execute().await
    }
}
