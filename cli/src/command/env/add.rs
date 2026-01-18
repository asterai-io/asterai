use crate::cli_ext::component::ComponentCliExt;
use crate::cli_ext::environment::EnvironmentCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;
use eyre::OptionExt;
use std::fs;

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
        write_environment_to_disk(&environment)?;
        Ok(())
    }
}

fn write_environment_to_disk(environment: &Environment) -> eyre::Result<()> {
    let serialized = serde_json::to_string(environment)?;
    let file_path = environment.local_disk_file_path();
    fs::create_dir_all(environment.local_disk_dir())?;
    fs::write(file_path, serialized)?;
    Ok(())
}
