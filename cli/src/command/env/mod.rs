use crate::command::env::init::init_env;
use crate::command::env::name_or_id::EnvNameOrId;
use crate::config::BIN_DIR;
use asterai_runtime::environment::Environment;
use asterai_runtime::resource::ResourceId;
use eyre::bail;
use std::path::PathBuf;
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
        let env_args = match action.as_str() {
            "init" => {
                let Some(env_name_or_id_string) = args.next() else {
                    bail!("missing env name/id");
                };
                let env_name_or_id = EnvNameOrId::from_str(&env_name_or_id_string)?;
                Self {
                    action: "init",
                    env_name_or_id: Some(env_name_or_id),
                    plugin_name: None,
                    env_var: None,
                    instance_id: None,
                }
            }
            _ => {
                bail!("unknown env action '{action}'")
            }
        };
        Ok(env_args)
    }

    pub async fn run(&self) -> eyre::Result<()> {
        match self.action {
            "init" => {
                let resource_id_string = self
                    .env_name_or_id
                    .as_ref()
                    .unwrap()
                    .id_with_local_namespace_fallback();
                let resource_id = ResourceId::from_str(&resource_id_string)?;
                init_env(resource_id).await?;
            }
            _ => {
                unimplemented!()
            }
        }
        Ok(())
    }
}

trait EnvironmentCliExt {
    fn local_disk_dir(&self) -> PathBuf;
    fn local_disk_file_path(&self) -> PathBuf;
    fn local_metadata_file_path(&self) -> PathBuf;
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
}
