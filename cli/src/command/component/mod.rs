use crate::command::common_flags::extract_common_flags;
use crate::command::component::build::BuildArgs;
use crate::command::component::delete::DeleteArgs;
use crate::command::component::init::InitArgs;
use crate::command::component::pkg::PkgArgs;
use crate::command::component::pull::PullArgs;
use crate::command::component::push::PushArgs;
use eyre::{bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

pub mod build;
pub mod delete;
pub mod init;
pub mod list;
pub mod pkg;
pub mod pull;
pub mod push;

#[derive(Debug)]
pub struct ComponentArgs {
    action: ComponentAction,
    #[allow(dead_code)]
    build_args: Option<BuildArgs>,
    pkg_args: Option<PkgArgs>,
    pull_args: Option<PullArgs>,
    push_args: Option<PushArgs>,
    init_args: Option<InitArgs>,
    delete_args: Option<DeleteArgs>,
    pub api_endpoint: String,
    #[allow(dead_code)]
    pub registry_endpoint: String,
}

#[derive(Debug, Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum ComponentAction {
    Build,
    Init,
    Ls,
    Pkg,
    Pull,
    Push,
    Rm,
}

impl ComponentArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(action_string) = args.next() else {
            bail!("missing component command action");
        };
        let action = ComponentAction::from_str(&action_string)
            .map_err(|_| eyre!("unknown component action"))?;
        // Collect remaining args and extract common flags.
        let remaining_args: Vec<String> = args.collect();
        let common = extract_common_flags(remaining_args)?;
        let api_endpoint = common.api_endpoint;
        let registry_endpoint = common.registry_endpoint;
        let args = common.remaining_args.into_iter();
        let none_args = Self {
            action,
            build_args: None,
            pkg_args: None,
            pull_args: None,
            push_args: None,
            init_args: None,
            delete_args: None,
            api_endpoint,
            registry_endpoint,
        };
        let command_args = match action {
            ComponentAction::Build => Self {
                build_args: Some(BuildArgs::parse(args)?),
                ..none_args
            },
            ComponentAction::Init => Self {
                init_args: Some(InitArgs::parse(args)?),
                ..none_args
            },
            ComponentAction::Ls => none_args,
            ComponentAction::Pkg => Self {
                pkg_args: Some(PkgArgs::parse(args)?),
                ..none_args
            },
            ComponentAction::Pull => Self {
                pull_args: Some(PullArgs::parse(args)?),
                ..none_args
            },
            ComponentAction::Push => Self {
                push_args: Some(PushArgs::parse(args)?),
                ..none_args
            },
            ComponentAction::Rm => Self {
                delete_args: Some(DeleteArgs::parse(args)?),
                ..none_args
            },
        };
        Ok(command_args)
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        match self.action {
            ComponentAction::Build => {
                self.build().await?;
            }
            ComponentAction::Init => {
                self.init()?;
            }
            ComponentAction::Ls => {
                self.list().await?;
            }
            ComponentAction::Pkg => {
                self.pkg().await?;
            }
            ComponentAction::Pull => {
                self.pull().await?;
            }
            ComponentAction::Push => {
                self.push().await?;
            }
            ComponentAction::Rm => {
                self.delete()?;
            }
        }
        Ok(())
    }

    fn delete(&self) -> eyre::Result<()> {
        let args = self
            .delete_args
            .as_ref()
            .ok_or_else(|| eyre!("no delete args"))?;
        args.execute()
    }
}
