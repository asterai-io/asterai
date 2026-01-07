use crate::cli_ext::component::ComponentCliExt;
use asterai_runtime::environment::Environment;
use asterai_runtime::plugin::PluginId;
use asterai_runtime::plugin::interface::PluginInterface;
use asterai_runtime::runtime::PluginRuntime;
use tokio::sync::mpsc;
use uuid::Uuid;

pub trait PluginRuntimeCliExt: Sized {
    async fn from_environment(environment: Environment) -> eyre::Result<Self>;
}

impl PluginRuntimeCliExt for PluginRuntime {
    async fn from_environment(environment: Environment) -> eyre::Result<Self> {
        let mut local_components = PluginInterface::local_list();
        let mut components = Vec::with_capacity(environment.components.len());
        for env_component in environment.components {
            let local_component_opt = find_component(&env_component.id(), &mut local_components);
            if let Some(local_component) = local_component_opt {
                components.push(local_component);
                continue;
            }
            // Local component not found, must fetch from registry.
            todo!()
        }
        // TODO update this according to new API.
        let app_id = Uuid::new_v4();
        let (plugin_output_tx, mut plugin_output_rx) = mpsc::channel(32);
        // Just drain the messages for now. TODO add to this fn's arg?
        tokio::spawn(async move { while let Some(_) = plugin_output_rx.recv().await {} });
        PluginRuntime::new(components, app_id, plugin_output_tx).await
    }
}

fn find_component(id: &PluginId, components: &mut Vec<PluginInterface>) -> Option<PluginInterface> {
    let Some(index) = components.iter().position(|c| c.plugin().id() == *id) else {
        return None;
    };
    Some(components.swap_remove(index))
}
