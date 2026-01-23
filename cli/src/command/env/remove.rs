use crate::cli_ext::environment::EnvironmentCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;
use eyre::OptionExt;

impl EnvArgs {
    pub fn remove(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let component = self.component.as_ref().ok_or_eyre("missing component")?;
        let mut environment = Environment::local_fetch(&resource_id)?;
        let removed = environment.remove_component(component.namespace(), component.name());
        if !removed {
            println!("component not found in environment");
        }
        environment.write_to_disk()?;
        Ok(())
    }
}
