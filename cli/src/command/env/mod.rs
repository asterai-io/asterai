use crate::command::resource_or_id::ResourceOrIdArg;
use asterai_runtime::component::Component;
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::{bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

mod call;
mod init;
mod inspect;
mod list;
mod run;

pub struct EnvArgs {
    action: EnvAction,
    env_resource_or_id: Option<ResourceOrIdArg>,
    plugin: Option<Component>,
    function: Option<String>,
    function_args: Vec<String>,
    env_var: Option<&'static str>,
}

#[derive(Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum EnvAction {
    Run,
    Call,
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
        let mut parse_env_name_or_id = || -> eyre::Result<ResourceOrIdArg> {
            let Some(env_name_or_id_string) = args.next() else {
                bail!("missing env name/id");
            };
            ResourceOrIdArg::from_str(&env_name_or_id_string).map_err(|e| eyre!(e))
        };
        let env_args = match action {
            action @ (EnvAction::Run | EnvAction::Inspect | EnvAction::Init) => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                plugin: None,
                function: None,
                function_args: vec![],
                env_var: None,
            },
            EnvAction::Call => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                plugin: Some(
                    Component::from_str(&args.next().expect("missing component name"))
                        .expect("invalid component name"),
                ),
                function: Some(args.next().expect("missing function")),
                function_args: args.collect::<Vec<_>>(),
                env_var: None,
            },
            EnvAction::List => Self {
                action,
                env_resource_or_id: None,
                plugin: None,
                function: None,
                function_args: vec![],
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
            EnvAction::Call => {
                self.call().await?;
            }
        }
        Ok(())
    }

    /// If a resource name is present, this returns the
    /// `ResourceId` using a fallback namespace if no namespace was given.
    /// Otherwise, if no resource name, it returns an `Err`.
    /// TODO should this remove the version if present, or let it fail?.
    fn resource_id(&self) -> eyre::Result<ResourceId> {
        let resource_id_string = self
            .env_resource_or_id
            .as_ref()
            .unwrap()
            .with_local_namespace_fallback();
        ResourceId::from_str(&resource_id_string).map_err(|e| eyre!(e))
    }

    // TODO if only resource_id available, should this get latest version?
    fn resource(&self) -> eyre::Result<Resource> {
        let resource_id_string = self
            .env_resource_or_id
            .as_ref()
            .unwrap()
            .with_local_namespace_fallback();
        Resource::from_str(&resource_id_string).map_err(|e| eyre!(e))
    }
}
