use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use crate::registry::RegistryClient;
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
            // Pull the component from the registry.
            let client = reqwest::Client::new();
            let registry =
                RegistryClient::new(&client, &self.api_endpoint, &self.registry_endpoint);
            registry
                .pull_component_optional_auth(&component, false)
                .await?;
        }
        let mut environment = LocalStore::fetch_environment(&resource_id)?;
        environment.add_component(&component);
        LocalStore::write_environment(&environment)?;
        Ok(())
    }
}
