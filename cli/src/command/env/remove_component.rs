use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use eyre::OptionExt;

impl EnvArgs {
    pub async fn remove_component(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let component_ref = self
            .component_ref
            .as_ref()
            .ok_or_eyre("missing component")?;
        let mut environment = LocalStore::fetch_environment(&resource_id)?;
        // Remove by namespace:name only (version not needed for removal).
        let removed = environment.remove_component(&component_ref.namespace, &component_ref.name);
        if !removed {
            println!("component not found in environment");
        }
        LocalStore::write_environment(&environment)?;
        Ok(())
    }
}
