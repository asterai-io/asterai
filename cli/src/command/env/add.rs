use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use asterai_runtime::component::Component;
use eyre::OptionExt;
use std::str::FromStr;

impl EnvArgs {
    pub async fn add(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let component_ref = self
            .component_ref
            .as_ref()
            .ok_or_eyre("missing component")?;
        // Resolve version if not specified.
        let resolved = component_ref.resolve(&self.api_endpoint).await?;
        let component = Component::from_str(&resolved)?;
        if !LocalStore::component_exists(&component) {
            // TODO: pull this once pull is implemented.
            eyre::bail!(
                "component {}:{}@{} does not exist locally",
                component.namespace(),
                component.name(),
                component.version()
            );
        }
        let mut environment = LocalStore::fetch_environment(&resource_id)?;
        environment.add_component(&component);
        LocalStore::write_environment(&environment)?;
        Ok(())
    }
}
