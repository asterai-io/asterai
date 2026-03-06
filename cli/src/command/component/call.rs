use crate::command::component::ComponentArgs;
use crate::command::env::call::call_on_runtime;
use crate::runtime::build_runtime;
use crate::version_resolver::ComponentRef;
use asterai_runtime::component::ComponentId;
use asterai_runtime::environment::Environment;
use eyre::OptionExt;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug)]
pub(super) struct CallArgs {
    component_ref: ComponentRef,
    function: String,
    function_args: Vec<String>,
}

impl CallArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let component_str = args.next().ok_or_eyre(
            "missing component reference \
             (e.g., namespace:name or namespace:name@version)",
        )?;
        let component_ref = ComponentRef::parse(&component_str)?;
        let function = args.next().ok_or_eyre("missing function name")?;
        let function_args: Vec<String> = args.collect();
        Ok(Self {
            component_ref,
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
        let resolved = self
            .component_ref
            .resolve(api_endpoint, registry_endpoint)
            .await?;
        let comp_id_str = format!(
            "{}:{}",
            self.component_ref.namespace, self.component_ref.name
        );
        let comp_id = ComponentId::from_str(&comp_id_str)?;
        let version = resolved
            .split_once('@')
            .map(|(_, v)| v.to_string())
            .unwrap_or_else(|| "0.0.0".to_string());
        let mut environment = Environment::new(
            self.component_ref.namespace.clone(),
            "component-call".to_string(),
            "0.0.0".to_string(),
        );
        environment.components.insert(comp_id_str, version);
        // Forward all system environment variables.
        for (key, value) in std::env::vars() {
            environment.vars.insert(key, value);
        }
        println!(
            "calling {}:{} function {}",
            self.component_ref.namespace, self.component_ref.name, self.function,
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
