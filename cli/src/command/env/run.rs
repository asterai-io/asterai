use crate::cli_ext::environment::EnvironmentCliExt;
use crate::cli_ext::plugin_runtime::PluginRuntimeCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;
use asterai_runtime::plugin::PluginId;
use asterai_runtime::runtime::PluginRuntime;
use std::str::FromStr;

impl EnvArgs {
    pub async fn run(&self) -> eyre::Result<()> {
        let resource = self.resource()?;
        println!("running env {resource}");
        let environment = Environment::local_fetch(&resource.id())?;
        let mut runtime = PluginRuntime::from_environment(environment).await?;
        // TODO: run all components concurrently instead of specifying one.
        let plugin_id = PluginId::from_str("lorenzo:http-server")?;
        runtime.call_run(&plugin_id).await?;
        Ok(())
    }
}
