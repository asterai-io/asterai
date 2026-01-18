use crate::cli_ext::component::ComponentCliExt;
use crate::cli_ext::environment::EnvironmentCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;
use eyre::OptionExt;

impl EnvArgs {
    pub fn add(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let component = self.component.as_ref().ok_or_eyre("missing component")?;
        if !component.check_does_exist_locally()? {
            // TODO: pull this once pull is implemented.
            unimplemented!("component does not exist locally");
        }
        let mut environment = Environment::local_fetch(&resource_id)?;
        environment.components.insert(component.clone());
        environment.write_to_disk()?;
        Ok(())
    }
}
