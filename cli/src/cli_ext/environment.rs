use crate::cli_ext::resource::ResourceCliExt;
use crate::cli_ext::resource_from_path;
use crate::cli_ext::resource_metadata::ResourceMetadataCliExt;
use crate::config::BIN_DIR;
use asterai_runtime::environment::Environment;
use asterai_runtime::resource::metadata::{ResourceKind, ResourceMetadata};
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::bail;
use std::fs;
use std::path::{Path, PathBuf};

pub trait EnvironmentCliExt: Sized {
    fn local_disk_dir(&self) -> PathBuf;
    fn local_disk_file_path(&self) -> PathBuf;
    fn local_metadata_file_path(&self) -> PathBuf;
    fn local_list() -> Vec<Self>;
    fn parse_local(path: &Path) -> eyre::Result<Self>;
    /// Fetches the most recent with the given ID.
    fn local_fetch(id: &ResourceId) -> eyre::Result<Self>;
}

impl EnvironmentCliExt for Environment {
    fn local_disk_dir(&self) -> PathBuf {
        BIN_DIR
            .join("resources")
            .join(self.resource.namespace())
            .join(format!(
                "{}@{}",
                self.resource.name(),
                self.resource.version()
            ))
    }

    fn local_disk_file_path(&self) -> PathBuf {
        self.local_disk_dir().join("env.json")
    }

    fn local_metadata_file_path(&self) -> PathBuf {
        self.local_disk_dir().join("metadata.json")
    }

    fn local_list() -> Vec<Self> {
        let resources = Resource::local_list();
        let mut envs = Vec::new();
        for resource_path in resources {
            let Ok(metadata) = ResourceMetadata::parse_local(&resource_path) else {
                eprintln!(
                    "ERROR: failed to parse metadata for environment at {}",
                    resource_path.to_str().unwrap_or_default()
                );
                continue;
            };
            if metadata.kind != ResourceKind::Environment {
                continue;
            }
            let env = match Environment::parse_local(&resource_path) {
                Ok(env) => env,
                Err(e) => {
                    eprintln!(
                        "ERROR: failed to parse environment at {} ({e:#?})",
                        resource_path.to_str().unwrap_or_default()
                    );
                    continue;
                }
            };
            envs.push(env);
        }
        envs
    }

    fn parse_local(path: &Path) -> eyre::Result<Self> {
        let resource = resource_from_path(path)?;
        let env_json_path = path.to_owned().join("env.json");
        let serialized = fs::read_to_string(&env_json_path)?;
        let environment: Environment = serde_json::from_str(&serialized)?;
        if environment.resource != resource {
            bail!("env.json resource does not match dir resource data");
        }
        Ok(environment)
    }

    fn local_fetch(id: &ResourceId) -> eyre::Result<Self> {
        let path = Resource::local_fetch_path(id)?;
        let environment = Self::parse_local(&path)?;
        Ok(environment)
    }
}
