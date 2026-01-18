use crate::cli_ext::environment::EnvironmentCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;

impl EnvArgs {
    pub fn init(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let resource = resource_id.with_version("0.0.0")?;
        let environment = Environment::new(resource);
        environment.write_to_disk()?;
        Ok(())
    }
}
