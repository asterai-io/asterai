use crate::command::env::init::init_env;
use crate::command::env::name_or_id::EnvNameOrId;
use crate::config::BIN_DIR;
use asterai_runtime::environment::Environment;
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::{bail, eyre};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod init;
mod name_or_id;

pub struct EnvArgs {
    action: &'static str,
    env_name_or_id: Option<EnvNameOrId>,
    plugin_name: Option<&'static str>,
    env_var: Option<&'static str>,
    instance_id: Option<&'static str>,
}

impl EnvArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(action) = args.next() else {
            bail!("missing env command action");
        };
        let mut parse_env_name_or_id = || -> eyre::Result<EnvNameOrId> {
            let Some(env_name_or_id_string) = args.next() else {
                bail!("missing env name/id");
            };
            EnvNameOrId::from_str(&env_name_or_id_string).map_err(|e| eyre!(e))
        };
        let env_args = match action.as_str() {
            "init" => Self {
                action: "init",
                env_name_or_id: Some(parse_env_name_or_id()?),
                plugin_name: None,
                env_var: None,
                instance_id: None,
            },
            "run" => Self {
                action: "run",
                env_name_or_id: Some(parse_env_name_or_id()?),
                plugin_name: None,
                env_var: None,
                instance_id: None,
            },
            _ => {
                bail!("unknown env action '{action}'")
            }
        };
        Ok(env_args)
    }

    pub async fn run(&self) -> eyre::Result<()> {
        let get_resource_id = || -> eyre::Result<ResourceId> {
            let resource_id_string = self
                .env_name_or_id
                .as_ref()
                .unwrap()
                .id_with_local_namespace_fallback();
            ResourceId::from_str(&resource_id_string).map_err(|e| eyre!(e))
        };
        match self.action {
            "init" => {
                init_env(get_resource_id()?).await?;
            }
            "run" => {
                let resource_id = get_resource_id()?;
                println!("running env {resource_id}");
                let environment = Environment::local_fetch(&resource_id)?;
                dbg!(&environment);
                // TODO
            }
            _ => {
                unimplemented!()
            }
        }
        Ok(())
    }
}

trait EnvironmentCliExt: Sized {
    fn local_disk_dir(&self) -> PathBuf;
    fn local_disk_file_path(&self) -> PathBuf;
    fn local_metadata_file_path(&self) -> PathBuf;
    fn local_list() -> Vec<PathBuf>;
    fn parse_local(path: &Path) -> eyre::Result<Self>;
    /// Fetches the most recent with the given ID.
    fn local_fetch(id: &ResourceId) -> eyre::Result<Self>;
}

impl EnvironmentCliExt for Environment {
    fn local_disk_dir(&self) -> PathBuf {
        let resource = self.resource();
        BIN_DIR
            .join("resources")
            .join(resource.namespace())
            .join(format!("{}@{}", resource.name(), resource.version()))
    }

    fn local_disk_file_path(&self) -> PathBuf {
        self.local_disk_dir().join("env.json")
    }

    fn local_metadata_file_path(&self) -> PathBuf {
        self.local_disk_dir().join("metadata.json")
    }

    fn local_list() -> Vec<PathBuf> {
        let resources_dir = BIN_DIR.join("resources");
        if !resources_dir.exists() {
            return Vec::new();
        }
        let mut paths = Vec::new();
        let Ok(namespaces) = fs::read_dir(&resources_dir) else {
            return Vec::new();
        };
        for namespace in namespaces.flatten() {
            let Ok(entries) = fs::read_dir(namespace.path()) else {
                continue;
            };
            paths.extend(entries.flatten().map(|e| e.path()));
        }
        paths
    }

    fn parse_local(path: &Path) -> eyre::Result<Self> {
        let resource = path_to_resource(path)?;
        let env_json_path = path.to_owned().join("env.json");
        let serialized = fs::read_to_string(&env_json_path)?;
        let environment: Environment = serde_json::from_str(&serialized)?;
        if *environment.resource() != resource {
            bail!("env.json resource does not match dir resource data");
        }
        Ok(environment)
    }

    fn local_fetch(id: &ResourceId) -> eyre::Result<Self> {
        let local_resources = Environment::local_list();
        let mut selected_resource_opt = None;
        for resource_path in local_resources {
            let resource = path_to_resource(&resource_path)?;
            if resource.id() != *id {
                continue;
            }
            match &selected_resource_opt {
                None => {
                    selected_resource_opt = Some((resource, resource_path));
                }
                Some((r, _)) => {
                    if resource.version() > r.version() {
                        selected_resource_opt = Some((resource, resource_path));
                    }
                }
            }
        }
        let Some((_, selected_resource_path)) = selected_resource_opt else {
            bail!("no resource found");
        };
        let environment = Self::parse_local(&selected_resource_path)?;
        Ok(environment)
    }
}

fn path_to_resource(path: &Path) -> eyre::Result<Resource> {
    let namespace = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .ok_or_else(|| eyre!("invalid namespace in path"))?;
    let name_version = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| eyre!("invalid name@version in path"))?;
    let (name, version) = name_version
        .split_once('@')
        .ok_or_else(|| eyre!("missing version separator '@' in path"))?;
    let resource_id = ResourceId::new_from_parts(namespace.to_owned(), name.to_owned())?;
    resource_id.with_version(&version)
}
