use crate::auth::Auth;
use crate::config::{API_URL, REGISTRY_URL};
use crate::local_store::LocalStore;
use crate::registry::RegistryClient;
use asterai_runtime::component::binary::ComponentBinary;
use asterai_runtime::component::{Component, ComponentId};
use asterai_runtime::environment::Environment;
use asterai_runtime::runtime::ComponentRuntime;
use eyre::{Context, OptionExt};
use std::str::FromStr;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Build a ComponentRuntime from an Environment.
pub async fn build_runtime(environment: Environment) -> eyre::Result<ComponentRuntime> {
    let mut local_components = LocalStore::list_components();
    let mut components = Vec::with_capacity(environment.components.len());
    // environment.components is HashMap<String, String>
    // where key is "namespace:name" and value is version.
    for (component_id_str, version) in &environment.components {
        let component_id = ComponentId::from_str(component_id_str).map_err(|e| {
            eyre::eyre!("failed to parse component ID '{}': {}", component_id_str, e)
        })?;
        let local_component_opt = find_component(&component_id, version, &mut local_components);
        if let Some(local_component) = local_component_opt {
            components.push(local_component);
            continue;
        }
        // Local component not found, fetch from registry.
        let component = pull_component(&component_id, version).await?;
        components.push(component);
    }
    // TODO: update this according to new API.
    let app_id = Uuid::new_v4();
    let (component_output_tx, mut component_output_rx) = mpsc::channel(32);
    // Just drain the messages for now. TODO: add to this fn's arg?
    tokio::spawn(async move { while component_output_rx.recv().await.is_some() {} });
    ComponentRuntime::new(
        components,
        app_id,
        component_output_tx,
        &environment.vars,
        &environment.metadata.namespace,
        &environment.metadata.name,
    )
    .await
}

async fn pull_component(id: &ComponentId, version: &str) -> eyre::Result<ComponentBinary> {
    let api_key = Auth::read_stored_api_key()
        .ok_or_eyre("API key not found. Run 'asterai auth login' to authenticate.")?;
    let component_ref = format!("{}:{}@{}", id.namespace(), id.name(), version);
    let component = Component::from_str(&component_ref)
        .map_err(|e| eyre::eyre!("invalid component reference: {}", e))?;
    println!("pulling component {}...", component_ref);
    let client = reqwest::Client::new();
    let registry = RegistryClient::new(&client, API_URL, REGISTRY_URL);
    let output_dir = registry
        .pull_component(Some(&api_key), &component, true)
        .await?;
    LocalStore::parse_component(&output_dir).wrap_err_with(|| {
        format!(
            "failed to parse pulled component at {}",
            output_dir.display()
        )
    })
}

fn find_component(
    id: &ComponentId,
    version: &str,
    components: &mut Vec<ComponentBinary>,
) -> Option<ComponentBinary> {
    let index = components.iter().position(|c| {
        c.component().id() == *id && c.component().version().to_string() == version
    })?;
    Some(components.swap_remove(index))
}
