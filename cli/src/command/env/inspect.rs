use crate::command::env::{EnvArgs, EnvironmentCliExt};
use asterai_runtime::environment::Environment;

impl EnvArgs {
    pub fn inspect(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let Ok(env) = Environment::local_fetch(&resource_id) else {
            println!("environment does not exist");
            return Ok(());
        };
        println!(
            "environment {env_resource} has {plugin_count} plugins",
            env_resource = env.resource(),
            plugin_count = env.plugins().len()
        );
        Ok(())
    }
}
