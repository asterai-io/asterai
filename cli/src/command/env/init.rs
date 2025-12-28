use crate::command::env::EnvironmentCliExt;
use asterai_runtime::environment::Environment;
use asterai_runtime::resource::ResourceId;
use std::fs;

trait EnvironmentCliInitExt {
    fn write_to_disk(&self) -> eyre::Result<()>;
}

pub async fn init_env(resource_id: ResourceId) -> eyre::Result<()> {
    let resource = resource_id.with_version("0.0.0")?;
    let environment = Environment::new(resource);
    environment.write_to_disk()?;
    Ok(())
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
