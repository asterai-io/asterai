use crate::command::resource_or_id::ResourceOrIdArg;
use asterai_runtime::component::Component;
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::{OptionExt, bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

mod add;
mod call;
mod delete;
mod init;
mod inspect;
mod list;
mod pull;
mod push;
mod remove;
mod run;

pub struct EnvArgs {
    action: EnvAction,
    env_resource_or_id: Option<ResourceOrIdArg>,
    component: Option<Component>,
    function: Option<String>,
    function_args: Vec<String>,
    env_var: Option<&'static str>,
    run_args: Option<run::RunArgs>,
    push_args: Option<push::PushArgs>,
    pull_args: Option<pull::PullArgs>,
    delete_args: Option<delete::DeleteArgs>,
}

#[derive(Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum EnvAction {
    Run,
    Call,
    Init,
    Inspect,
    List,
    Add,
    Remove,
    Push,
    Pull,
    Delete,
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
            EnvAction::Run => Self {
                action,
                env_resource_or_id: None,
                component: None,
                function: None,
                function_args: vec![],
                env_var: None,
                run_args: Some(run::RunArgs::parse(args)?),
                push_args: None,
                pull_args: None,
                delete_args: None,
            },
            action @ (EnvAction::Inspect | EnvAction::Init) => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                component: None,
                function: None,
                function_args: vec![],
                env_var: None,
                run_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
            },
            EnvAction::Call => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                component: Some(
                    Component::from_str(&args.next().expect("missing component name"))
                        .expect("invalid component name"),
                ),
                function: Some(args.next().expect("missing function")),
                function_args: args.collect::<Vec<_>>(),
                env_var: None,
                run_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
            },
            EnvAction::Add => {
                let env_resource_or_id = parse_env_name_or_id()?;
                let component = parse_component_flag(&mut args)?;
                Self {
                    action,
                    env_resource_or_id: Some(env_resource_or_id),
                    component: Some(component),
                    function: None,
                    function_args: vec![],
                    env_var: None,
                    run_args: None,
                    push_args: None,
                    pull_args: None,
                    delete_args: None,
                }
            }
            EnvAction::Remove => {
                let env_resource_or_id = parse_env_name_or_id()?;
                let component = parse_component_flag(&mut args)?;
                Self {
                    action,
                    env_resource_or_id: Some(env_resource_or_id),
                    component: Some(component),
                    function: None,
                    function_args: vec![],
                    env_var: None,
                    run_args: None,
                    push_args: None,
                    pull_args: None,
                    delete_args: None,
                }
            }
            EnvAction::List => Self {
                action,
                env_resource_or_id: None,
                component: None,
                function: None,
                function_args: vec![],
                env_var: None,
                run_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
            },
            EnvAction::Push => Self {
                action,
                env_resource_or_id: None,
                component: None,
                function: None,
                function_args: vec![],
                env_var: None,
                run_args: None,
                push_args: Some(push::PushArgs::parse(args)?),
                pull_args: None,
                delete_args: None,
            },
            EnvAction::Pull => Self {
                action,
                env_resource_or_id: None,
                component: None,
                function: None,
                function_args: vec![],
                env_var: None,
                run_args: None,
                push_args: None,
                pull_args: Some(pull::PullArgs::parse(args)?),
                delete_args: None,
            },
            EnvAction::Delete => Self {
                action,
                env_resource_or_id: None,
                component: None,
                function: None,
                function_args: vec![],
                env_var: None,
                run_args: None,
                push_args: None,
                pull_args: None,
                delete_args: Some(delete::DeleteArgs::parse(args)?),
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
                let args = self.run_args.as_ref().ok_or_eyre("no run args")?;
                args.execute().await?;
            }
            EnvAction::Inspect => {
                self.inspect()?;
            }
            EnvAction::List => {
                self.list().await?;
            }
            EnvAction::Call => {
                self.call().await?;
            }
            EnvAction::Add => {
                self.add()?;
            }
            EnvAction::Remove => {
                self.remove()?;
            }
            EnvAction::Push => {
                self.push().await?;
            }
            EnvAction::Pull => {
                self.pull().await?;
            }
            EnvAction::Delete => {
                self.delete().await?;
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

fn parse_component_flag(args: &mut impl Iterator<Item = String>) -> eyre::Result<Component> {
    let mut component = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--component" => {
                let component_string =
                    args.next().ok_or_eyre("missing value for component flag")?;
                let parsed_component =
                    Component::from_str(&component_string).map_err(|e| eyre!(e))?;
                component = Some(parsed_component);
            }
            _ => bail!("unknown flag: {}", arg),
        }
    }
    component.ok_or_eyre("missing component flag")
}

impl EnvArgs {
    pub async fn push(&self) -> eyre::Result<()> {
        let args = self.push_args.as_ref().ok_or_eyre("no push args")?;
        args.execute().await
    }

    pub async fn pull(&self) -> eyre::Result<()> {
        let args = self.pull_args.as_ref().ok_or_eyre("no pull args")?;
        args.execute().await
    }

    pub async fn delete(&self) -> eyre::Result<()> {
        let args = self.delete_args.as_ref().ok_or_eyre("no delete args")?;
        args.execute().await
    }
}
