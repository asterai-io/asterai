use crate::command::env::{EnvArgs, EnvironmentCliExt};
use asterai_runtime::environment::Environment;

impl EnvArgs {
    pub async fn run(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        println!("running env {resource_id}");
        let environment = Environment::local_fetch(&resource_id)?;
        dbg!(&environment);
        // TODO
        Ok(())
    }
}
