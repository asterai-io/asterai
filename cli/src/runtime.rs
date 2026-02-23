use crate::auth::Auth;
use crate::config::{API_URL, REGISTRY_URL};
use crate::local_store::LocalStore;
use crate::registry::RegistryClient;
use crate::version_resolver;
use asterai_runtime::component::binary::ComponentBinary;
use asterai_runtime::component::{Component, ComponentId};
use asterai_runtime::environment::Environment;
use asterai_runtime::environment::deps;
use asterai_runtime::runtime::ComponentRuntime;
use eyre::Context;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Build a ComponentRuntime from an Environment.
pub async fn build_runtime(
    environment: Environment,
    allow_dirs: &[PathBuf],
) -> eyre::Result<ComponentRuntime> {
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
    // Auto-resolve missing dependencies.
    resolve_dependencies(&mut components, &mut local_components).await?;
    // Warn about imported interfaces exported by multiple components.
    // Components are sorted alphabetically for instantiation, so the
    // first provider in the sorted list is the one the linker will use.
    for (interface, providers) in deps::conflicting_exports(&components) {
        let default = &providers[0];
        eprintln!(
            "warning: interface {} is exported by multiple components: {}. \
             {} was picked as the default implementor.",
            interface,
            providers.join(", "),
            default,
        );
    }
    if !allow_dirs.is_empty() {
        println!("allowed directories:");
        for dir in allow_dirs {
            println!("  {}", dir.display());
        }
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
        allow_dirs,
        &environment.metadata.namespace,
        &environment.metadata.name,
    )
    .await
}

/// Iteratively resolves unsatisfied component imports by pulling missing
/// dependencies from the registry. Runs until all imports are satisfied
/// or a dependency cannot be found.
async fn resolve_dependencies(
    components: &mut Vec<ComponentBinary>,
    local_components: &mut Vec<ComponentBinary>,
) -> eyre::Result<()> {
    const MAX_ITERATIONS: usize = 100;
    for _ in 0..MAX_ITERATIONS {
        let missing = deps::unsatisfied_import_packages(components);
        if missing.is_empty() {
            return Ok(());
        }
        for resource_id in &missing {
            let version = version_resolver::resolve_latest_version(
                resource_id.namespace(),
                resource_id.name(),
                API_URL,
                REGISTRY_URL,
            )
            .await?;
            println!("  auto-resolved dependency: {}@{}", resource_id, version);
            let component_id = ComponentId::from_str(&resource_id.to_string())?;
            let version_str = version.to_string();
            let local_opt = find_component(&component_id, &version_str, local_components);
            let binary = match local_opt {
                Some(local) => local,
                None => pull_component(&component_id, &version_str).await?,
            };
            components.push(binary);
        }
    }
    Err(eyre::eyre!(
        "dependency resolution exceeded {} iterations — possible cycle",
        MAX_ITERATIONS
    ))
}

async fn pull_component(id: &ComponentId, version: &str) -> eyre::Result<ComponentBinary> {
    let api_key = Auth::read_stored_api_key();
    let component_ref = format!("{}:{}@{}", id.namespace(), id.name(), version);
    let component = Component::from_str(&component_ref)
        .map_err(|e| eyre::eyre!("invalid component reference: {}", e))?;
    println!("pulling component {}...", component_ref);
    let client = reqwest::Client::new();
    let registry = RegistryClient::new(&client, API_URL, REGISTRY_URL);
    let output_dir = registry
        .pull_component(api_key.as_deref(), &component, true)
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

pub fn expand_tilde(path: &str, home: Option<&str>) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match home {
            Some(h) => PathBuf::from(h).join(rest),
            None => PathBuf::from(path),
        },
        None => match path == "~" {
            true => home
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(path)),
            false => PathBuf::from(path),
        },
    }
}
