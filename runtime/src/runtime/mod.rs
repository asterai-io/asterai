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
pub mod output;
pub mod parsing;
pub(crate) mod std_out_err;
mod wasm_instance;
mod wit_bindings;

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
        let http_route_table =
            build_http_route_table(&engine, env_vars, preopened_dirs, env_namespace, env_name)?;
        Ok(Self {
            app_id,
            engine,
            http_route_table: Arc::new(http_route_table),
        })
    }

    pub fn http_route_table(&self) -> Arc<HttpRouteTable> {
        self.http_route_table.clone()
    }

    pub fn component_interfaces(&self) -> Vec<ComponentBinary> {
        self.engine
            .instances()
            .iter()
            .map(|i| i.component_interface.clone())
            .collect()
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
    ) -> Option<ComponentFunctionInterface> {
        self.component_interfaces().iter().find_map(|interface| {
            if interface.component().id() != *component_id {
                return None;
            }
            let functions = interface.get_functions();
            functions.into_iter().find(|f| {
                if let Some(package_name) = &package_name_opt {
                    let is_same_package = f.package_name.name == package_name.name
                        && f.package_name.namespace == package_name.namespace;
                    // When specified, ensure the package name matches.
                    if !is_same_package {
                        return false;
                    }
                    let is_compatible_version = package_name.version.is_none()
                        || package_name.version == f.package_name.version;
                    if !is_compatible_version {
                        return false;
                    }
                }
                &f.name == function_name
            })
        })
    }

    /// Call all the `run` functions concurrently, which is commonly defined by `wasi:cli/run`
    /// to run CLI components, on all components that implement it.
    pub async fn run(&mut self) -> eyre::Result<()> {
        let mut funcs = Vec::new();
        for instance in &self.engine.instances {
            let component = instance.component_interface.component().clone();
            let run_function_opt = self.find_function(
                &component.id(),
                &CLI_RUN_FUNCTION_NAME,
                // Do not specify a package, as usually this is only implemented once.
                // e.g. a common target would be wasi:cli@0.2.0
                None,
            );
            let Some(run_function) = run_function_opt else {
                // Skip components that don't implement run.
                continue;
            };
            let func = run_function.get_func(&mut self.engine.store, &instance.instance)?;
            funcs.push((func, run_function.name, component));
        }
        self.engine
            .store
            .run_concurrent(async |a| {
                for (func, func_name, component) in funcs {
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
                }
            })
            .await
            .map_err(|e| eyre!(e))?;
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
