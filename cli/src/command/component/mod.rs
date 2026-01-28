use crate::command::component::build::BuildArgs;
use crate::command::component::delete::DeleteArgs;
use crate::command::component::init::InitArgs;
use crate::command::component::pkg::PkgArgs;
use crate::command::component::pull::PullArgs;
use crate::command::component::push::PushArgs;
use crate::config::{API_URL, API_URL_STAGING, REGISTRY_URL, REGISTRY_URL_STAGING};
use eyre::{OptionExt, bail, eyre};
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
    List,
    Pkg,
    Pull,
    Push,
    Delete,
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
        let (api_endpoint, registry_endpoint, filtered_args) =
            Self::extract_common_flags(remaining_args)?;
        let args = filtered_args.into_iter();
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
            ComponentAction::List => none_args,
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
            ComponentAction::Delete => Self {
                delete_args: Some(DeleteArgs::parse(args)?),
                ..none_args
            },
        };
        Ok(command_args)
    }

    /// Extract common flags (-e/--endpoint, -r/--registry, -s/--staging) from args.
    fn extract_common_flags(args: Vec<String>) -> eyre::Result<(String, String, Vec<String>)> {
        let mut api_endpoint = API_URL.to_string();
        let mut registry_endpoint = REGISTRY_URL.to_string();
        let mut staging = false;
        let mut filtered = Vec::new();
        let mut iter = args.into_iter().peekable();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--endpoint" | "-e" => {
                    api_endpoint = iter
                        .next()
                        .ok_or_eyre("missing value for --endpoint flag")?;
                }
                "--registry" | "-r" => {
                    registry_endpoint = iter
                        .next()
                        .ok_or_eyre("missing value for --registry flag")?;
                }
                "--staging" | "-s" => {
                    staging = true;
                }
                _ => filtered.push(arg),
            }
        }
        // Staging flag overrides explicit endpoints.
        if staging {
            api_endpoint = API_URL_STAGING.to_string();
            registry_endpoint = REGISTRY_URL_STAGING.to_string();
        }
        Ok((api_endpoint, registry_endpoint, filtered))
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        match self.action {
            ComponentAction::Build => {
                self.build().await?;
            }
            ComponentAction::Init => {
                self.init()?;
            }
            ComponentAction::List => {
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
