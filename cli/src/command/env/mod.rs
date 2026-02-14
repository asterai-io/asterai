use crate::command::common_flags::extract_common_flags;
use crate::command::env::cp::CpArgs;
use crate::command::env::delete::DeleteArgs;
use crate::command::env::pull::PullArgs;
use crate::command::env::push::PushArgs;
use crate::command::env::run::RunArgs;
use crate::command::env::set_var::SetVarArgs;
use crate::command::resource_or_id::ResourceOrIdArg;
use crate::version_resolver::ComponentRef;
use asterai_runtime::resource::ResourceId;
use eyre::{OptionExt, bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

mod add_component;
mod call;
pub(crate) mod call_api;
mod cp;
mod delete;
mod edit;
mod init;
mod inspect;
mod list;
mod pull;
mod push;
mod remove_component;
mod run;
mod set_var;

pub struct EnvArgs {
    action: EnvAction,
    env_resource_or_id: Option<ResourceOrIdArg>,
    component_arg: Option<ResourceOrIdArg>,
    component_ref: Option<ComponentRef>,
    function: Option<String>,
    function_args: Vec<String>,
    run_args: Option<RunArgs>,
    set_var_args: Option<SetVarArgs>,
    push_args: Option<PushArgs>,
    pull_args: Option<PullArgs>,
    delete_args: Option<DeleteArgs>,
    cp_args: Option<CpArgs>,
    should_open_editor: bool,
    pub api_endpoint: String,
    pub registry_endpoint: String,
    pub allow_dirs: Vec<std::path::PathBuf>,
}

#[derive(Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum EnvAction {
    Run,
    Call,
    Init,
    Inspect,
    Ls,
    AddComponent,
    RemoveComponent,
    SetVar,
    Push,
    Pull,
    Delete,
    Edit,
    Cp,
}

impl EnvArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(action_string) = args.next() else {
            bail!("missing env command action");
        };
        let action =
            EnvAction::from_str(&action_string).map_err(|_| eyre!("unknown env action"))?;
        // Collect remaining args and extract common flags.
        let remaining_args: Vec<String> = args.collect();
        let common = extract_common_flags(remaining_args)?;
        let api_endpoint = common.api_endpoint;
        let registry_endpoint = common.registry_endpoint;
        let allow_dirs = common.allow_dirs;
        let mut args = common.remaining_args.into_iter();
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
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: Some(RunArgs::parse(args, allow_dirs.clone())?),
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
            EnvAction::Init => {
                let env_resource_or_id = parse_env_name_or_id()?;
                let mut should_open_editor = false;
                for arg in args {
                    match arg.as_str() {
                        "--edit" => should_open_editor = true,
                        other => bail!("unexpected argument: {}", other),
                    }
                }
                Self {
                    action,
                    env_resource_or_id: Some(env_resource_or_id),
                    component_arg: None,
                    component_ref: None,
                    function: None,
                    function_args: vec![],
                    run_args: None,
                    set_var_args: None,
                    push_args: None,
                    pull_args: None,
                    delete_args: None,
                    cp_args: None,
                    should_open_editor,
                    api_endpoint,
                    registry_endpoint,
                    allow_dirs: allow_dirs.clone(),
                }
            }
            action @ (EnvAction::Inspect | EnvAction::Edit) => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
            EnvAction::Call => {
                let env = parse_env_name_or_id()?;
                let comp_str = args.next().ok_or_eyre("missing component name")?;
                let comp_arg = ResourceOrIdArg::from_str(&comp_str).map_err(|e| eyre!(e))?;
                let function = args.next().ok_or_eyre("missing function")?;
                Self {
                    action,
                    env_resource_or_id: Some(env),
                    component_arg: Some(comp_arg),
                    component_ref: None,
                    function: Some(function),
                    function_args: args.collect::<Vec<_>>(),
                    run_args: None,
                    set_var_args: None,
                    push_args: None,
                    pull_args: None,
                    delete_args: None,
                    cp_args: None,
                    should_open_editor: false,
                    api_endpoint,
                    registry_endpoint,
                    allow_dirs: allow_dirs.clone(),
                }
            }
            EnvAction::AddComponent => {
                let env_resource_or_id = parse_env_name_or_id()?;
                let component_string = args.next().ok_or_eyre(
                    "missing component (e.g. namespace:component or namespace:component@version)",
                )?;
                let component_ref = ComponentRef::parse(&component_string)?;
                Self {
                    action,
                    env_resource_or_id: Some(env_resource_or_id),
                    component_arg: None,
                    component_ref: Some(component_ref),
                    function: None,
                    function_args: vec![],
                    run_args: None,
                    set_var_args: None,
                    push_args: None,
                    pull_args: None,
                    delete_args: None,
                    cp_args: None,
                    should_open_editor: false,
                    api_endpoint,
                    registry_endpoint,
                    allow_dirs: allow_dirs.clone(),
                }
            }
            EnvAction::RemoveComponent => {
                let env_resource_or_id = parse_env_name_or_id()?;
                let component_string = args.next().ok_or_eyre(
                    "missing component (e.g. namespace:component or namespace:component@version)",
                )?;
                let component_ref = ComponentRef::parse(&component_string)?;
                Self {
                    action,
                    env_resource_or_id: Some(env_resource_or_id),
                    component_arg: None,
                    component_ref: Some(component_ref),
                    function: None,
                    function_args: vec![],
                    run_args: None,
                    set_var_args: None,
                    push_args: None,
                    pull_args: None,
                    delete_args: None,
                    cp_args: None,
                    should_open_editor: false,
                    api_endpoint,
                    registry_endpoint,
                    allow_dirs: allow_dirs.clone(),
                }
            }
            EnvAction::Ls => Self {
                action,
                env_resource_or_id: None,
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
            EnvAction::SetVar => Self {
                action,
                env_resource_or_id: None,
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: Some(SetVarArgs::parse(args)?),
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
            EnvAction::Push => Self {
                action,
                env_resource_or_id: None,
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: Some(PushArgs::parse(args)?),
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
            EnvAction::Pull => Self {
                action,
                env_resource_or_id: None,
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: Some(PullArgs::parse(args)?),
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
            EnvAction::Delete => Self {
                action,
                env_resource_or_id: None,
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: Some(DeleteArgs::parse(args)?),
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
            EnvAction::Cp => Self {
                action,
                env_resource_or_id: None,
                component_arg: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: Some(CpArgs::parse(args)?),
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
                allow_dirs: allow_dirs.clone(),
            },
        };
        Ok(env_args)
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        match self.action {
            EnvAction::Init => {
                self.init()?;
                if self.should_open_editor {
                    self.edit()?;
                }
            }
            EnvAction::Run => {
                let args = self.run_args.as_ref().ok_or_eyre("no run args")?;
                args.execute(&self.api_endpoint, &self.registry_endpoint)
                    .await?;
            }
            EnvAction::Inspect => {
                self.inspect()?;
            }
            EnvAction::Ls => {
                self.list().await?;
            }
            EnvAction::Call => {
                self.call().await?;
            }
            EnvAction::AddComponent => {
                self.add_component().await?;
            }
            EnvAction::RemoveComponent => {
                self.remove_component().await?;
            }
            EnvAction::SetVar => {
                self.set_var()?;
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
            EnvAction::Edit => {
                self.edit()?;
            }
            EnvAction::Cp => {
                self.cp()?;
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

    pub async fn push(&self) -> eyre::Result<()> {
        let args = self.push_args.as_ref().ok_or_eyre("no push args")?;
        args.execute(&self.api_endpoint).await
    }

    pub async fn pull(&self) -> eyre::Result<()> {
        let args = self.pull_args.as_ref().ok_or_eyre("no pull args")?;
        args.execute(&self.api_endpoint, &self.registry_endpoint)
            .await
    }

    pub async fn delete(&self) -> eyre::Result<()> {
        let args = self.delete_args.as_ref().ok_or_eyre("no delete args")?;
        args.execute(&self.api_endpoint).await
    }

    pub fn cp(&self) -> eyre::Result<()> {
        let args = self.cp_args.as_ref().ok_or_eyre("no cp args")?;
        args.execute()
    }
}
