use crate::command::component::ComponentArgs;
use crate::command::component::push::parse_package_name;
use crate::command::env::call::call_on_runtime;
use crate::language;
use crate::runtime::{build_runtime, build_runtime_with};
use crate::version_resolver::ComponentRef;
use asterai_runtime::component::binary::ComponentBinary;
use asterai_runtime::component::{Component, ComponentId};
use asterai_runtime::environment::Environment;
use eyre::{OptionExt, bail};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug)]
pub(super) struct CallArgs {
    component_ref: Option<ComponentRef>,
    is_local_project: bool,
    function: String,
    function_args: Vec<String>,
}

impl CallArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let component_str = args.next().ok_or_eyre(
            "missing component reference \
             (e.g., namespace:name, namespace:name@version, or . for current dir)",
        )?;
        let is_local_project = component_str == ".";
        let component_ref = match is_local_project {
            true => None,
            false => Some(ComponentRef::parse(&component_str)?),
        };
        let function = args.next().ok_or_eyre("missing function name")?;
        let function_args: Vec<String> = args.collect();
        Ok(Self {
            component_ref,
            is_local_project,
            function,
            function_args,
        })
    }

    async fn execute(
        &self,
        api_endpoint: &str,
        registry_endpoint: &str,
        allow_dirs: &[PathBuf],
    ) -> eyre::Result<()> {
        match self.is_local_project {
            true => self.execute_local(allow_dirs).await,
            false => {
                self.execute_remote(api_endpoint, registry_endpoint, allow_dirs)
                    .await
            }
        }
    }

    async fn execute_remote(
        &self,
        api_endpoint: &str,
        registry_endpoint: &str,
        allow_dirs: &[PathBuf],
    ) -> eyre::Result<()> {
        let comp_ref = self.component_ref.as_ref().unwrap();
        let resolved = comp_ref.resolve(api_endpoint, registry_endpoint).await?;
        let comp_id_str = format!("{}:{}", comp_ref.namespace, comp_ref.name);
        let comp_id = ComponentId::from_str(&comp_id_str)?;
        let version = resolved
            .split_once('@')
            .map(|(_, v)| v.to_string())
            .unwrap_or_else(|| "0.0.0".to_string());
        let mut environment = Environment::new(
            comp_ref.namespace.clone(),
            "component-call".to_string(),
            "0.0.0".to_string(),
        );
        environment.components.insert(comp_id_str, version);
        for (key, value) in std::env::vars() {
            environment.vars.insert(key, value);
        }
        println!(
            "calling {}:{} function {}",
            comp_ref.namespace, comp_ref.name, self.function,
        );
        let mut runtime = build_runtime(environment, allow_dirs).await?;
        let result = call_on_runtime(
            &mut runtime,
            &comp_id,
            self.function.clone(),
            &self.function_args,
        )
        .await?;
        if let Some(output) = result {
            println!("{output}");
        }
        Ok(())
    }

    async fn execute_local(&self, allow_dirs: &[PathBuf]) -> eyre::Result<()> {
        let cwd = std::env::current_dir()?;
        let lang = language::detect(&cwd)
            .ok_or_eyre("current directory is not a recognised component project")?;
        let component_wasm_path = lang.get_component_wasm_path(&cwd)?;
        if !component_wasm_path.exists() {
            bail!(
                "component not built yet (expected {}). Run: asterai component build",
                component_wasm_path.display()
            );
        }
        let package_wasm_path = lang.get_package_wasm_path(&cwd);
        if !package_wasm_path.exists() {
            bail!(
                "package.wasm not found (expected {}). Run: asterai component build",
                package_wasm_path.display()
            );
        }
        let pkg_bytes = std::fs::read(&package_wasm_path)?;
        let pkg_name = parse_package_name(&pkg_bytes)?;
        let version = pkg_name
            .version
            .as_ref()
            .ok_or_eyre("package.wasm has no version")?;
        let comp_ref = format!("{}:{}@{}", pkg_name.namespace, pkg_name.name, version);
        let component = Component::from_str(&comp_ref)
            .map_err(|e| eyre::eyre!("invalid component reference: {e}"))?;
        let comp_id = component.id();
        let comp_id_str = format!("{}:{}", pkg_name.namespace, pkg_name.name);
        let component_bytes = std::fs::read(&component_wasm_path)?;
        let mut binary = ComponentBinary::from_component_bytes(component, component_bytes)?;
        binary.apply_package_docs(&pkg_bytes)?;
        let mut environment = Environment::new(
            pkg_name.namespace.clone(),
            "component-call".to_string(),
            "0.0.0".to_string(),
        );
        environment
            .components
            .insert(comp_id_str, version.to_string());
        for (key, value) in std::env::vars() {
            environment.vars.insert(key, value);
        }
        println!(
            "calling {}:{} function {}",
            pkg_name.namespace, pkg_name.name, self.function,
        );
        let mut runtime = build_runtime_with(environment, allow_dirs, vec![binary]).await?;
        let result = call_on_runtime(
            &mut runtime,
            &comp_id,
            self.function.clone(),
            &self.function_args,
        )
        .await?;
        if let Some(output) = result {
            println!("{output}");
        }
        Ok(())
    }
}

impl ComponentArgs {
    pub async fn call(&self) -> eyre::Result<()> {
        let args = self.call_args.as_ref().ok_or_eyre("no call args")?;
        args.execute(
            &self.api_endpoint,
            &self.registry_endpoint,
            &self.allow_dirs,
        )
        .await
    }
}
