use crate::command::component::delete::DeleteArgs;
use crate::command::component::init::InitArgs;
use crate::command::component::pkg::PkgArgs;
use crate::command::component::pull::PullArgs;
use crate::command::component::push::PushArgs;
use eyre::{bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

pub mod delete;
pub mod init;
pub mod list;
pub mod pkg;
pub mod pull;
pub mod push;

#[derive(Debug)]
pub struct ComponentArgs {
    action: ComponentAction,
    pkg_args: Option<PkgArgs>,
    pull_args: Option<PullArgs>,
    push_args: Option<PushArgs>,
    init_args: Option<InitArgs>,
    delete_args: Option<DeleteArgs>,
}

#[derive(Debug, Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum ComponentAction {
    Init,
    List,
    Pkg,
    Pull,
    Push,
    Delete,
}

impl ComponentArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(action_string) = args.next() else {
            bail!("missing env command action");
        };
        let action =
            ComponentAction::from_str(&action_string).map_err(|_| eyre!("unknown env action"))?;
        let command_args = match action {
            ComponentAction::Init => Self {
                action,
                pkg_args: None,
                pull_args: None,
                push_args: None,
                init_args: Some(InitArgs::parse(args)?),
                delete_args: None,
            },
            ComponentAction::List => Self {
                action,
                pkg_args: None,
                pull_args: None,
                push_args: None,
                init_args: None,
                delete_args: None,
            },
            ComponentAction::Pkg => Self {
                action,
                pkg_args: Some(PkgArgs::parse(args)?),
                pull_args: None,
                push_args: None,
                init_args: None,
                delete_args: None,
            },
            ComponentAction::Pull => Self {
                action,
                pkg_args: None,
                pull_args: Some(PullArgs::parse(args)?),
                push_args: None,
                init_args: None,
                delete_args: None,
            },
            ComponentAction::Push => Self {
                action,
                pkg_args: None,
                pull_args: None,
                push_args: Some(PushArgs::parse(args)?),
                init_args: None,
                delete_args: None,
            },
            ComponentAction::Delete => Self {
                action,
                pkg_args: None,
                pull_args: None,
                push_args: None,
                init_args: None,
                delete_args: Some(DeleteArgs::parse(args)?),
            },
        };
        Ok(command_args)
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        match self.action {
            ComponentAction::Init => {
                self.init()?;
            }
            ComponentAction::List => {
                self.list()?;
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
            ComponentAction::Delete => {
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
