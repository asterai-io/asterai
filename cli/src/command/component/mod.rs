use crate::command::component::pkg::PkgArgs;
use crate::command::resource_or_id::ResourceOrIdArg;
use asterai_runtime::resource::ResourceId;
use eyre::{bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

pub mod list;
pub mod pkg;

#[derive(Debug)]
pub struct ComponentArgs {
    action: ComponentAction,
    component_resource_or_id: Option<ResourceOrIdArg>,
    plugin_name: Option<&'static str>,
    env_var: Option<&'static str>,
    pkg_args: Option<PkgArgs>,
}

#[derive(Debug, Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum ComponentAction {
    List,
    Pkg,
}

impl ComponentArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(action_string) = args.next() else {
            bail!("missing env command action");
        };
        let action =
            ComponentAction::from_str(&action_string).map_err(|_| eyre!("unknown env action"))?;
        let command_args = match action {
            ComponentAction::List => Self {
                action,
                component_resource_or_id: None,
                plugin_name: None,
                env_var: None,
                pkg_args: None,
            },
            ComponentAction::Pkg => Self {
                action,
                component_resource_or_id: None,
                plugin_name: None,
                env_var: None,
                pkg_args: Some(PkgArgs::parse(args)?),
            },
        };
        Ok(command_args)
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        match self.action {
            ComponentAction::List => {
                self.list()?;
            }
            ComponentAction::Pkg => {
                self.pkg().await?;
            }
        }
        Ok(())
    }

    /// If a resource name is present, this returns the
    /// `ResourceId` using a fallback namespace if no namespace was given.
    /// Otherwise, if no resource name, it returns an `Err`.
    /// TODO also add method for Resource (including version
    /// TODO reduce duplication with env module?
    fn resource_id(&self) -> eyre::Result<ResourceId> {
        let resource_id_string = self
            .component_resource_or_id
            .as_ref()
            .unwrap()
            .with_local_namespace_fallback();
        ResourceId::from_str(&resource_id_string).map_err(|e| eyre!(e))
    }
}
