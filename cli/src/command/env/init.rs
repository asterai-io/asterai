use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use asterai_runtime::environment::Environment;

impl EnvArgs {
    pub fn init(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let environment = Environment::new(
            resource_id.namespace().to_string(),
            resource_id.name().to_string(),
            "0.0.0".to_string(),
        );
        LocalStore::write_environment(&environment)?;
        Ok(())
    }
}
