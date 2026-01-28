use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use eyre::OptionExt;

impl EnvArgs {
    pub fn add(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let component = self.component.as_ref().ok_or_eyre("missing component")?;
        if !LocalStore::component_exists(component) {
            // TODO: pull this once pull is implemented.
            unimplemented!("component does not exist locally");
        }
        let mut environment = LocalStore::fetch_environment(&resource_id)?;
        environment.add_component(component);
        LocalStore::write_environment(&environment)?;
        Ok(())
    }
}
