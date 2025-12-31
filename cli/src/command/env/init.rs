use crate::command::env::{EnvArgs, EnvironmentCliExt};
use asterai_runtime::environment::Environment;
use std::fs;

trait EnvironmentCliInitExt {
    fn write_to_disk(&self) -> eyre::Result<()>;
}

impl EnvArgs {
    pub fn init(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let resource = resource_id.with_version("0.0.0")?;
        let environment = Environment::new(resource);
        environment.write_to_disk()?;
        Ok(())
    }
}
impl EnvironmentCliInitExt for Environment {
    fn write_to_disk(&self) -> eyre::Result<()> {
        let serialized = serde_json::to_string(&self)?;
        let file_path = self.local_disk_file_path();
        fs::create_dir_all(self.local_disk_dir())?;
        fs::write(file_path, serialized)?;
        Ok(())
    }
}
