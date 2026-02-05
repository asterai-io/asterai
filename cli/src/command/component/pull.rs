use crate::command::component::ComponentArgs;
use crate::config::ARTIFACTS_DIR;
use crate::registry::RegistryClient;
use crate::version_resolver::ComponentRef;
use asterai_runtime::resource::Resource;
use asterai_runtime::resource::metadata::ResourceKind;
use eyre::{Context, OptionExt, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug)]
pub(super) struct PullArgs {
    /// Component reference (version optional, will resolve to latest if omitted).
    component_ref: ComponentRef,
    /// Output directory (defaults to local resource storage).
    output: Option<String>,
}

impl PullArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut component_ref: Option<ComponentRef> = None;
        let mut output: Option<String> = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-o" | "--output" => {
                    output = Some(args.next().ok_or_eyre("missing value for output flag")?);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if component_ref.is_some() {
                        bail!("unexpected argument: {}", other);
                    }
                    component_ref = Some(ComponentRef::parse(other).wrap_err(
                        "invalid component reference \
                         (expected namespace:name or namespace:name@version)",
                    )?);
                }
            }
        }
        let component_ref = component_ref.ok_or_eyre(
            "missing component reference \
            (e.g., namespace:component or namespace:component@version)",
        )?;
        Ok(Self {
            component_ref,
            output,
        })
    }

    async fn execute(&self, api_url: &str, registry_url: &str) -> eyre::Result<()> {
        // Resolve version if not specified.
        let resolved = self.component_ref.resolve(api_url).await?;
        let component = Resource::from_str(&resolved)?;
        let namespace = component.namespace();
        let name = component.name();
        let version = component.version().to_string();
        let repo_name = format!("{}/{}", namespace, name);
        let tag = &version;
        println!("pulling {}@{}", repo_name, tag);
        let client = reqwest::Client::new();
        let registry = RegistryClient::new(&client, api_url, registry_url);
        let token = registry.get_token(None, &repo_name).await?;
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
        args.execute(&self.api_endpoint, &self.registry_endpoint)
            .await
    }
}
