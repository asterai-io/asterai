use crate::command::env::resource_or_id::EnvResourceOrId;
use asterai_runtime::resource::ResourceId;
use eyre::{bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

mod init;
mod inspect;
mod list;
mod resource_or_id;
mod run;

pub struct EnvArgs {
    action: EnvAction,
    env_resource_or_id: Option<EnvResourceOrId>,
    plugin_name: Option<&'static str>,
    env_var: Option<&'static str>,
}

#[derive(Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum EnvAction {
    Run,
    Init,
    Inspect,
    List,
}

impl EnvArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(action_string) = args.next() else {
            bail!("missing env command action");
        };
        let action =
            EnvAction::from_str(&action_string).map_err(|_| eyre!("unknown env action"))?;
        let mut parse_env_name_or_id = || -> eyre::Result<EnvResourceOrId> {
            let Some(env_name_or_id_string) = args.next() else {
                bail!("missing env name/id");
            };
            EnvResourceOrId::from_str(&env_name_or_id_string).map_err(|e| eyre!(e))
        };
        let env_args = match action {
            action @ (EnvAction::Run | EnvAction::Inspect | EnvAction::Init) => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                plugin_name: None,
                env_var: None,
            },
            EnvAction::List => Self {
                action,
                env_resource_or_id: None,
                plugin_name: None,
                env_var: None,
            },
        };
        Ok(env_args)
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        match self.action {
            EnvAction::Init => {
                self.init()?;
            }
            EnvAction::Run => {
                self.run().await?;
            }
            EnvAction::Inspect => {
                self.inspect()?;
            }
            EnvAction::List => {
                self.list()?;
            }
        }
        Ok(())
    }

    /// If a resource name is present, this returns the
    /// `ResourceId` using a fallback namespace if no namespace was given.
    /// Otherwise, if no resource name, it returns an `Err`.
    /// TODO also add method for Resource (including version).
    fn resource_id(&self) -> eyre::Result<ResourceId> {
        let resource_id_string = self
            .env_resource_or_id
            .as_ref()
            .unwrap()
            .with_local_namespace_fallback();
        ResourceId::from_str(&resource_id_string).map_err(|e| eyre!(e))
    }
}
