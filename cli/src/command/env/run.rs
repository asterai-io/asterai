use crate::cli_ext::environment::EnvironmentCliExt;
use crate::cli_ext::plugin_runtime::PluginRuntimeCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;
use asterai_runtime::runtime::PluginRuntime;

impl EnvArgs {
    pub async fn run(&self) -> eyre::Result<()> {
        let resource = self.resource()?;
        println!("running env {resource}");
        let environment = Environment::local_fetch(&resource.id())?;
        let mut runtime = PluginRuntime::from_environment(environment).await?;
        runtime.run().await?;
        Ok(())
    }
}
