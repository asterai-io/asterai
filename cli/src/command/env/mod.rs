use crate::command::resource_or_id::ResourceOrIdArg;
use crate::config::{API_URL, REGISTRY_URL};
use crate::version_resolver::ComponentRef;
use asterai_runtime::component::Component;
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::{OptionExt, bail, eyre};
use std::str::FromStr;
use strum_macros::EnumString;

mod add;
mod call;
mod cp;
mod delete;
mod edit;
mod init;
mod inspect;
mod list;
mod pull;
mod push;
mod remove;
mod run;
mod set_var;

pub struct EnvArgs {
    action: EnvAction,
    env_resource_or_id: Option<ResourceOrIdArg>,
    component: Option<Component>,
    component_ref: Option<ComponentRef>,
    function: Option<String>,
    function_args: Vec<String>,
    run_args: Option<run::RunArgs>,
    set_var_args: Option<set_var::SetVarArgs>,
    push_args: Option<push::PushArgs>,
    pull_args: Option<pull::PullArgs>,
    delete_args: Option<delete::DeleteArgs>,
    cp_args: Option<cp::CpArgs>,
    should_open_editor: bool,
    pub api_endpoint: String,
    pub registry_endpoint: String,
}

#[derive(Copy, Clone, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum EnvAction {
    Run,
    Call,
    Init,
    Inspect,
    List,
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
        let (api_endpoint, registry_endpoint, filtered_args) =
            Self::extract_common_flags(remaining_args)?;
        let mut args = filtered_args.into_iter();
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
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: Some(run::RunArgs::parse(args)?),
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
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
                    component: None,
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
                }
            }
            action @ (EnvAction::Inspect | EnvAction::Edit) => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                component: None,
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
            },
            EnvAction::Call => Self {
                action,
                env_resource_or_id: Some(parse_env_name_or_id()?),
                component: Some(
                    Component::from_str(&args.next().expect("missing component name"))
                        .expect("invalid component name"),
                ),
                component_ref: None,
                function: Some(args.next().expect("missing function")),
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
            },
            EnvAction::AddComponent => {
                let env_resource_or_id = parse_env_name_or_id()?;
                let component_string = args.next().ok_or_eyre(
                    "missing component (e.g. namespace:component or namespace:component@version)",
                )?;
                let component_ref = ComponentRef::parse(&component_string)?;
                Self {
                    action,
                    env_resource_or_id: Some(env_resource_or_id),
                    component: None,
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
                    component: None,
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
                }
            }
            EnvAction::List => Self {
                action,
                env_resource_or_id: None,
                component: None,
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
            },
            EnvAction::SetVar => Self {
                action,
                env_resource_or_id: None,
                component: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: Some(set_var::SetVarArgs::parse(args)?),
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
            },
            EnvAction::Push => Self {
                action,
                env_resource_or_id: None,
                component: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: Some(push::PushArgs::parse(args)?),
                pull_args: None,
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
            },
            EnvAction::Pull => Self {
                action,
                env_resource_or_id: None,
                component: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: Some(pull::PullArgs::parse(args)?),
                delete_args: None,
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
            },
            EnvAction::Delete => Self {
                action,
                env_resource_or_id: None,
                component: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: Some(delete::DeleteArgs::parse(args)?),
                cp_args: None,
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
            },
            EnvAction::Cp => Self {
                action,
                env_resource_or_id: None,
                component: None,
                component_ref: None,
                function: None,
                function_args: vec![],
                run_args: None,
                set_var_args: None,
                push_args: None,
                pull_args: None,
                delete_args: None,
                cp_args: Some(cp::CpArgs::parse(args)?),
                should_open_editor: false,
                api_endpoint,
                registry_endpoint,
            },
        };
        Ok(env_args)
    }

    /// Extract common flags (-e/--endpoint, -r/--registry, -s/--staging) from args.
    /// Returns (api_endpoint, registry_endpoint, remaining_args).
    /// If --staging is set, it overrides the endpoints with staging URLs.
    fn extract_common_flags(args: Vec<String>) -> eyre::Result<(String, String, Vec<String>)> {
        use crate::config::{API_URL_STAGING, REGISTRY_URL_STAGING};
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
            EnvAction::List => {
                self.list().await?;
            }
            EnvAction::Call => {
                self.call().await?;
            }
            EnvAction::AddComponent => {
                self.add().await?;
            }
            EnvAction::RemoveComponent => {
                self.remove().await?;
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

    // TODO if only resource_id available, should this get latest version?
    fn resource(&self) -> eyre::Result<Resource> {
        let resource_id_string = self
            .env_resource_or_id
            .as_ref()
            .unwrap()
            .with_local_namespace_fallback();
        Resource::from_str(&resource_id_string).map_err(|e| eyre!(e))
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
