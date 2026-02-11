use crate::component::ComponentId;
use crate::component::binary::ComponentBinary;
use crate::component::function_interface::ComponentFunctionInterface;
use crate::component::function_name::ComponentFunctionName;
use crate::component::wit::ComponentInterface;
use crate::runtime::http::{HttpRoute, HttpRouteTable};
use crate::runtime::output::{ComponentFunctionOutput, ComponentOutput};
use crate::runtime::wasm_instance::{
    ComponentRuntimeEngine, call_wasm_component_function_concurrent,
};
use crate::runtime::ws::WsManager;
use derive_getters::Getters;
use eyre::eyre;
use log::{error, trace};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;
pub use wasmtime::component::Val;
use wasmtime_wasi_http::bindings::ProxyPre;
use wit_parser::PackageName;

mod entry;
pub mod env;
pub mod http;
mod link_components;
pub mod output;
pub mod parsing;
pub(crate) mod std_out_err;
mod wasm_instance;
mod wit_bindings;
pub mod ws;
mod ws_entry;

// The `run/run` function, commonly defined by `wasi:cli`.
static CLI_RUN_FUNCTION_NAME: Lazy<ComponentFunctionName> = Lazy::new(|| ComponentFunctionName {
    interface: Some("run".to_owned()),
    name: "run".to_owned(),
});

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SerializableVal {
    pub name: Option<String>,
    pub val: Val,
}

/// The runtime which holds all instantiated components
/// within an app.
/// TODO: rename to EnvironmentRuntime?
#[derive(Getters)]
pub struct ComponentRuntime {
    app_id: Uuid,
    #[getter(skip)]
    engine: ComponentRuntimeEngine,
    #[getter(skip)]
    http_route_table: Arc<HttpRouteTable>,
    #[getter(skip)]
    ws_manager: Option<Arc<WsManager>>,
}

impl ComponentRuntime {
    pub async fn new(
        components: Vec<ComponentBinary>,
        // TODO: change app ID for resource ID?
        app_id: Uuid,
        component_output_tx: mpsc::Sender<ComponentOutput>,
        env_vars: &HashMap<String, String>,
        preopened_dirs: &[PathBuf],
        env_namespace: &str,
        env_name: &str,
    ) -> eyre::Result<Self> {
        let engine = ComponentRuntimeEngine::new(
            components,
            app_id,
            component_output_tx,
            env_vars,
            preopened_dirs,
        )
        .await?;
        let ws_manager = {
            let store = engine.store.lock().await;
            store
                .data()
                .runtime_data
                .as_ref()
                .and_then(|r| r.ws_manager.clone())
        };
        let http_route_table =
            build_http_route_table(&engine, env_vars, preopened_dirs, env_namespace, env_name)?;
        Ok(Self {
            app_id,
            engine,
            http_route_table: Arc::new(http_route_table),
            ws_manager,
        })
    }

    pub fn http_route_table(&self) -> Arc<HttpRouteTable> {
        self.http_route_table.clone()
    }

    pub fn ws_manager(&self) -> Option<Arc<WsManager>> {
        self.ws_manager.clone()
    }

    pub fn component_interfaces(&self) -> Vec<ComponentBinary> {
        self.engine
            .instances()
            .iter()
            .map(|i| i.component_interface.clone())
            .collect()
    }

    /// Returns the WIT `Resolve` for the given component.
    pub fn resolve_for(&self, comp_id: &ComponentId) -> Option<wit_parser::Resolve> {
        self.engine
            .instances()
            .iter()
            .find(|i| i.component_interface.component().id() == *comp_id)
            .map(|i| i.component_interface.wit().resolve().clone())
    }

    pub async fn call_function(
        &mut self,
        component_manifest_function: ComponentFunctionInterface,
        inputs: &[Val],
    ) -> eyre::Result<Option<ComponentOutput>> {
        let output_opt = self
            .engine
            .call(component_manifest_function, inputs)
            .await?;
        Ok(output_opt)
    }

    pub fn find_function(
        &self,
        component_id: &ComponentId,
        function_name: &ComponentFunctionName,
        package_name_opt: Option<PackageName>,
    ) -> eyre::Result<Option<ComponentFunctionInterface>> {
        let functions = self.get_component_functions(component_id);
        let exact = functions.iter().find(|f| {
            if !Self::matches_package(&f.package_name, &package_name_opt) {
                return false;
            }
            &f.name == function_name
        });
        if let Some(found) = exact {
            return Ok(Some(found.clone()));
        }
        // The full function name was passed, and an exact match
        // was not found.
        if function_name.interface.is_some() {
            return Ok(None);
        }
        // Only the function name was passed (no interface).
        // Try matching by function name alone across all interfaces.
        // This is allowed as long as there is no ambiguity
        // (i.e. multiple functions of the same name across multiple interfaces).
        let matches: Vec<_> = functions
            .into_iter()
            .filter(|f| {
                if !Self::matches_package(&f.package_name, &package_name_opt) {
                    return false;
                }
                f.name.name == function_name.name
            })
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches.into_iter().next().unwrap())),
            _ => {
                let names: Vec<String> = matches.iter().map(|f| f.name.to_string()).collect();
                Err(eyre!(
                    "function '{}' is ambiguous, found in multiple \
                     interfaces: {}. Use the full interface/function format.",
                    function_name.name,
                    names.join(", "),
                ))
            }
        }
    }

    fn get_component_functions(
        &self,
        component_id: &ComponentId,
    ) -> Vec<ComponentFunctionInterface> {
        self.component_interfaces()
            .iter()
            .filter(|i| i.component().id() == *component_id)
            .flat_map(|i| i.get_functions())
            .collect()
    }

    fn matches_package(package_name: &PackageName, filter: &Option<PackageName>) -> bool {
        let Some(filter) = filter else {
            return true;
        };
        let is_same =
            package_name.name == filter.name && package_name.namespace == filter.namespace;
        if !is_same {
            return false;
        }
        filter.version.is_none() || filter.version == package_name.version
    }

    /// Call all the `run` functions, which is commonly defined by `wasi:cli/run`,
    /// on all components that implement it.
    ///
    /// **Limitation:** Components are run sequentially, not concurrently.
    /// Each component's `run` must complete before the next one starts.
    /// This means a long-lived `run` (e.g. a server loop) will block
    /// subsequent components from ever executing.
    ///
    /// The sequential design exists because `run_concurrent` holds
    /// `&mut Store` (locking the shared `Arc<Mutex<Store>>`), which
    /// prevents WS callbacks from acquiring the store. Running each
    /// component separately releases the lock between calls.
    ///
    /// To restore true concurrency, all concurrent guest execution
    /// (component runs + WS callbacks) would need to share a single
    /// `run_concurrent` scope via the `Accessor`, with WS dispatch
    /// routed through a channel into that scope.
    /// See: https://github.com/asterai-io/asterai/issues/67
    pub async fn run(&mut self) -> eyre::Result<()> {
        let funcs = {
            let mut store = self.engine.store.lock().await;
            let mut funcs = Vec::new();
            for instance in &self.engine.instances {
                let component = instance.component_interface.component().clone();
                let run_function_opt = self.find_function(
                    &component.id(),
                    &CLI_RUN_FUNCTION_NAME,
                    // Do not specify a package, as usually this is only implemented once.
                    // e.g. a common target would be wasi:cli@0.2.0
                    None,
                )?;
                let Some(run_function) = run_function_opt else {
                    // Skip components that don't implement run.
                    continue;
                };
                let func = run_function.get_func(&mut *store, &instance.instance)?;
                funcs.push((func, run_function.name, component));
            }
            funcs
        };
        // Run each component separately so the store lock is released between
        // calls, allowing WS callbacks to proceed while other components run.
        for (func, func_name, component) in funcs {
            let mut store = self.engine.store.lock().await;
            let last_component = store
                .data()
                .runtime_data
                .as_ref()
                .unwrap()
                .last_component
                .clone();
            *last_component.lock().unwrap() = Some(component.clone());
            store
                .run_concurrent(async |a| {
                    let result = call_wasm_component_function_concurrent(
                        &func,
                        &func_name,
                        a,
                        &[],
                        &mut [Val::Bool(false)],
                        component,
                    )
                    .await;
                    if let Err(e) = result {
                        error!("{e:#?}");
                    }
                })
                .await
                .map_err(|e| eyre!(e))?;
        }
        Ok(())
    }
}

impl ComponentOutput {
    pub fn from(
        val_opt: Option<Val>,
        component_function_interface: ComponentFunctionInterface,
        component_response_to_agent_opt: Option<String>,
    ) -> Option<ComponentOutput> {
        let function_output_opt = val_opt.and_then(|val| {
            component_function_interface
                .clone()
                .output_type
                .map(|type_def| {
                    let name = type_def.name.clone();
                    ComponentFunctionOutput {
                        type_def,
                        value: SerializableVal { name, val },
                        function_interface: component_function_interface,
                    }
                })
        });
        Some(Self {
            function_output_opt,
            component_response_to_agent_opt,
        })
    }
}

fn has_incoming_handler(component_binary: &ComponentBinary) -> bool {
    component_binary
        .exported_interfaces()
        .iter()
        .any(|e| e.name.starts_with("wasi:http/incoming-handler"))
}

fn build_http_route_table(
    engine: &ComponentRuntimeEngine,
    env_vars: &HashMap<String, String>,
    preopened_dirs: &[PathBuf],
    env_namespace: &str,
    env_name: &str,
) -> eyre::Result<HttpRouteTable> {
    let mut routes = HashMap::new();
    for entry in &engine.compiled_components {
        if !has_incoming_handler(&entry.component_binary) {
            continue;
        }
        let component = entry.component_binary.component();
        trace!(
            "detected incoming-handler on {}:{}",
            component.namespace(),
            component.name()
        );
        let instance_pre = engine
            .linker
            .instantiate_pre(&entry.component)
            .map_err(|e| eyre!("{e:#?}"))?;
        let proxy_pre = ProxyPre::new(instance_pre).map_err(|e| eyre!("{e:#?}"))?;
        let route_key = format!("{}/{}", component.namespace(), component.name());
        let http_route = HttpRoute {
            component: component.clone(),
            proxy_pre,
        };
        routes.insert(route_key, Arc::new(http_route));
    }
    Ok(HttpRouteTable::new(
        routes,
        env_vars.clone(),
        preopened_dirs.to_vec(),
        env_namespace.to_string(),
        env_name.to_string(),
    ))
}

impl Debug for ComponentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentRuntime")
            .field("app_id", &self.app_id)
            .finish()
    }
}

#[allow(async_fn_in_trait)]
pub trait ComponentFunctionInterfaceExt {
    async fn call(
        self,
        runtime: &mut ComponentRuntime,
        inputs: &[Val],
    ) -> eyre::Result<Option<ComponentOutput>>;
}

impl ComponentFunctionInterfaceExt for ComponentFunctionInterface {
    async fn call(
        self,
        runtime: &mut ComponentRuntime,
        inputs: &[Val],
    ) -> eyre::Result<Option<ComponentOutput>> {
        runtime.call_function(self, inputs).await
    }
}
