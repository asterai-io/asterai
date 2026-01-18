use crate::cli_ext::component_runtime::ComponentRuntimeCliExt;
use crate::cli_ext::environment::EnvironmentCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;
use asterai_runtime::runtime::ComponentRuntime;

impl EnvArgs {
    pub async fn run(&self) -> eyre::Result<()> {
        let resource = self.resource()?;
        println!("running env {resource}");
        let environment = Environment::local_fetch(&resource.id())?;
        let mut runtime = ComponentRuntime::from_environment(environment).await?;
        runtime.run().await?;
        Ok(())
    }
}
