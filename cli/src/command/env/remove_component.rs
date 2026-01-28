use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use eyre::OptionExt;

impl EnvArgs {
    pub fn remove_component(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let component = self.component.as_ref().ok_or_eyre("missing component")?;
        let mut environment = LocalStore::fetch_environment(&resource_id)?;
        let removed = environment.remove_component(component.namespace(), component.name());
        if !removed {
            println!("component not found in environment");
        }
        LocalStore::write_environment(&environment)?;
        Ok(())
    }
}
