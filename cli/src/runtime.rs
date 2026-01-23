use crate::local_store::LocalStore;
use asterai_runtime::component::ComponentId;
use asterai_runtime::component::interface::ComponentBinary;
use asterai_runtime::environment::Environment;
use asterai_runtime::runtime::ComponentRuntime;
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

        let local_component_opt = find_component(&component_id, &mut local_components);
        if let Some(local_component) = local_component_opt {
            // TODO: validate version matches.
            components.push(local_component);
            continue;
        }

        // Local component not found, must fetch from registry.
        todo!(
            "component {}@{} not found locally, need to fetch from registry",
            component_id_str,
            version
        )
    }

    // TODO: update this according to new API.
    let app_id = Uuid::new_v4();
    let (component_output_tx, mut component_output_rx) = mpsc::channel(32);

    // Just drain the messages for now. TODO: add to this fn's arg?
    tokio::spawn(async move { while component_output_rx.recv().await.is_some() {} });

    ComponentRuntime::new(components, app_id, component_output_tx).await
}

fn find_component(
    id: &ComponentId,
    components: &mut Vec<ComponentBinary>,
) -> Option<ComponentBinary> {
    let index = components.iter().position(|c| c.component().id() == *id)?;
    Some(components.swap_remove(index))
}
