use crate::component::Component;
use crate::component::function_name::ComponentFunctionName;
use crate::component::interface::{ComponentBinary, ComponentFunctionInterface};
use crate::runtime::entry::add_asterai_host_to_linker;
use crate::runtime::env::{HostEnv, HostEnvRuntimeData};
use crate::runtime::output::ComponentOutput;
use crate::runtime::std_out_err::{ComponentStderr, ComponentStdout};
use eyre::{Context, eyre};
use log::trace;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;
use wasmtime::component::*;
use wasmtime::{AsContext, AsContextMut, Config, Engine, Store, StoreContext, StoreContextMut};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::p2::add_to_linker_async;
use wasmtime_wasi_http::{WasiHttpCtx, add_only_http_to_linker_async};

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.async_support(true);
    Engine::new(&config).unwrap()
});

pub struct ComponentRuntimeEngine {
    pub(super) store: Store<StoreState>,
    pub(super) instances: Vec<ComponentRuntimeInstance>,
}

#[derive(Clone)]
pub struct ComponentRuntimeInstance {
    // TODO add app_plugin_id here to make it easily accessible
    // in host entry functions.
    pub component_interface: ComponentBinary,
    pub app_id: Uuid,
    pub(super) instance: Instance,
}

// #[derive(Debug)]
// pub struct StoreState {}
pub type StoreState = HostEnv;

impl ComponentRuntimeEngine {
    pub async fn new(
        mut components: Vec<ComponentBinary>,
        app_id: Uuid,
        component_output_tx: mpsc::Sender<ComponentOutput>,
        env_vars: &HashMap<String, String>,
    ) -> eyre::Result<Self> {
        let engine = &ENGINE;
        let last_component = Arc::new(Mutex::new(None));
        // Create a WASI context and put it in a Store; all instances in the store
        // share this context. `WasiCtxBuilder` provides a number of ways to
        // configure what the target program will have access to.
        let mut wasi_ctx_builder = WasiCtxBuilder::new();
        wasi_ctx_builder
            .stdout(ComponentStdout { app_id })
            .stderr(ComponentStderr { app_id })
            .inherit_network();
        // Inject environment variables from the environment config.
        for (key, value) in env_vars {
            wasi_ctx_builder.env(key, value);
        }
        let wasi_ctx = wasi_ctx_builder.build();
        let http_ctx = WasiHttpCtx::new();
        let table = ResourceTable::new();
        let host_env = HostEnv {
            runtime_data: None,
            wasi_ctx,
            http_ctx,
            table,
            component_output_tx,
        };
        let mut store = Store::new(engine, host_env);
        let mut linker = Linker::new(engine);
        // Prevent "defined twice" errors.
        linker.allow_shadowing(true);
        add_to_linker_async(&mut linker).map_err(|e| eyre!(e))?;
        add_only_http_to_linker_async(&mut linker).map_err(|e| eyre!(e))?;
        add_asterai_host_to_linker(&mut linker)?;
        let mut instances = Vec::new();
        // Sort by ascending order of imports count,
        // so that components with no dependencies are added first.
        components.sort_by(|a, b| b.get_imports_count().cmp(&a.get_imports_count()));
        // TODO fix this up. See if possible to link before instantiation,
        // using Linker::define or Linker::define_instance -- or maybe Grok hallucinated it?
        for interface in components.into_iter() {
            trace!("@ interface {}", interface.component().id());
            trace!("imports count: {}", interface.get_imports_count());
            let component = interface.fetch_compiled_component(engine).await?;
            trace!("fetched compiled component");
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .map_err(|e| eyre!("{e:#?}"))
                .with_context(|| "failed to initiate component")?;
            let instance = ComponentRuntimeInstance {
                component_interface: interface,
                app_id,
                instance,
            };
            instance.add_to_linker(&mut linker, &mut store)?;
            instances.push(instance);
        }
        let mut runtime_engine = Self {
            store,
            instances: instances.clone(),
        };
        runtime_engine.store.data_mut().runtime_data = Some(HostEnvRuntimeData {
            app_id,
            instances,
            last_component,
            component_response_to_agent: None,
        });
        Ok(runtime_engine)
    }

    pub fn instances(&self) -> &[ComponentRuntimeInstance] {
        &self.instances
    }

    pub async fn call(
        &mut self,
        function_interface: ComponentFunctionInterface,
        inputs: &[Val],
    ) -> eyre::Result<Option<ComponentOutput>> {
        // This is an uninitialised vec of the results.
        let mut results = function_interface.new_results_vec();
        self.call_raw(&function_interface, &inputs, &mut results)
            .await?;
        let output_opt =
            parse_component_output(self.store.as_context(), results, function_interface);
        Ok(output_opt)
    }

    /// Makes a function call to a WASM component.
    async fn call_raw(
        &mut self,
        function_interface: &ComponentFunctionInterface,
        args: &[Val],
        results: &mut [Val],
    ) -> eyre::Result<()> {
        let instance_opt = self.instances.iter().find(|instance| {
            instance.component_interface.component().id() == function_interface.component.id()
        });
        let Some(instance) = instance_opt else {
            return Err(eyre!(
                "instance not found for function '{:#?}'",
                function_interface
            ));
        };
        let functions = instance.component_interface.get_functions();
        let function_opt = functions
            .into_iter()
            .find(|f| &f.name == function_interface.name());
        let Some(function) = function_opt else {
            return Err(eyre!("function not found in instance"));
        };
        let func = function.get_func(&mut self.store, &instance.instance)?;
        let component = function.component.clone();
        call_wasm_component_function(
            &func,
            &function.name,
            self.store.as_context_mut(),
            args,
            results,
            component,
        )
        .await?;
        Ok(())
    }
}

impl ComponentRuntimeInstance {
    /// Add all plugin exports to the linker.
    pub fn add_to_linker(
        &self,
        linker: &mut Linker<HostEnv>,
        mut store: impl AsContextMut,
    ) -> eyre::Result<()> {
        trace!(
            "adding component to linker: {}",
            self.component_interface.component()
        );
        let functions = self.component_interface.get_functions();
        let mut functions_by_instance: HashMap<String, Vec<ComponentFunctionInterface>> =
            HashMap::new();
        for function in functions {
            let Some(instance_export_name) = function.get_instance_export_name() else {
                // Functions at the world level do not need to be
                // declared here, as they are available directly
                // though the root instance via `get_func`.
                trace!("skipping root function '{}'", function.name);
                continue;
            };
            trace!("aggregating export function {instance_export_name}: {function:#?}");
            let instance_functions = functions_by_instance
                .entry(instance_export_name)
                .or_default();
            instance_functions.push(function);
        }
        for (instance_name, functions) in functions_by_instance {
            // Instances need to only be fetched once, otherwise if they are
            // fetched multiple times it will override all functions previously
            // registered, which is why the export name and functions are
            // aggregated into the hashmap above.
            let mut exported_instance = linker
                .instance(&instance_name)
                .map_err(|e| eyre!("{e:#?}"))?;
            for function in functions {
                let func = function
                    .get_func(&mut store, &self.instance)
                    .map_err(|e| eyre!("{e:#?}"))?;
                let component = self.component_interface.component().clone();
                let func_name_cloned = function.name.clone();
                let func_name = function.name.clone();
                trace!("adding function to linker (export {instance_name}): '{func_name_cloned}'");
                exported_instance
                    .func_new_async(
                        &func_name_cloned.name,
                        move |mut store, _, params, mut results| {
                            let component_cloned = component.clone();
                            let func_name = func_name.clone();
                            let function_cloned = function.clone();
                            Box::new(async move {
                                call_wasm_component_function(
                                    &func,
                                    &func_name,
                                    store.as_context_mut(),
                                    params,
                                    &mut results,
                                    component_cloned,
                                )
                                .await
                                .unwrap();
                                let output_opt = parse_component_output(
                                    store.as_context(),
                                    results.to_vec(),
                                    function_cloned,
                                );
                                if let Some(output) = output_opt {
                                    // Forward output to channel.
                                    // This output is not from the directly called function,
                                    // but from an internal function call somewhere in the stack.
                                    store.data().component_output_tx.send(output).await.unwrap();
                                }
                                Ok(())
                            })
                        },
                    )
                    .map_err(|e| eyre!("{e:#?}"))?;
            }
        }
        Ok(())
    }
}

async fn call_wasm_component_function<'a>(
    func: &Func,
    func_name: &ComponentFunctionName,
    mut store: StoreContextMut<'a, HostEnv>,
    args: &[Val],
    results: &mut [Val],
    component: Component,
) -> eyre::Result<()> {
    let component_id = component.id().clone();
    trace!("calling function' from component '{}'", component.id());
    set_last_component(component, &mut store);
    func.call_async(&mut store, args, results)
        .await
        .map_err(|e| {
            eyre!(
                "failed to call func' from component '{}': {e:#?}",
                component_id,
            )
        })?;
    func.post_return_async(&mut store)
        .await
        .map_err(|e| eyre!(e))?;
    Ok(())
}

pub async fn call_wasm_component_function_concurrent<'a>(
    func: &Func,
    func_name: &ComponentFunctionName,
    accessor: &Accessor<HostEnv>,
    args: &[Val],
    results: &mut [Val],
    component: Component,
) -> eyre::Result<()> {
    let component_id = component.id().clone();
    trace!("calling function' from component '{}'", component.id());
    func.call_concurrent(accessor, args, results)
        .await
        .map_err(|e| {
            eyre!(
                "failed to call func' from component '{}': {e:#?}",
                component_id,
            )
        })?;
    Ok(())
}

/// Set this component as the last one called.
/// This is necessary for knowing the component ID in case the function
/// called accesses host functions such as logging or any other part of the host API.
/// TODO: this doesnt currently work with concurrent calls, decide whether to keep it.
fn set_last_component(component: Component, store: &mut StoreContextMut<HostEnv>) {
    *store
        .data_mut()
        .runtime_data
        .as_mut()
        .unwrap()
        .last_component
        .lock()
        .unwrap() = Some(component);
}

fn parse_component_output(
    store: StoreContext<HostEnv>,
    results: Vec<Val>,
    interface: ComponentFunctionInterface,
) -> Option<ComponentOutput> {
    let val_opt = results.into_iter().next();
    let component_response_to_agent_opt = store
        .data()
        .runtime_data
        .as_ref()
        .and_then(|r| r.component_response_to_agent.clone());
    // If the app is an agent, then the return value of functions will be serialized to text
    // and sent to the agent unless the function calls the API method `send_response_to_agent`
    // which can be used to override the return value.
    //
    // This overriding by `send_response_to_agent` can be useful if the function returns a large
    // output that the agent does not need (such as to be consumed by the client/front-end),
    // allowing the plugin to re-word the output as natural language while keeping the structured
    // output usable for the client to consume it.
    //
    // The input to `send_response_to_agent` is represented by `component_response_to_agent_opt` here.
    // TODO: add API method for `send_no_response_to_agent` to prevent any response from being sent.
    // TODO: revisit above if app is not an agent (e.g. blueprint app)
    let component_response_to_agent_opt =
        component_response_to_agent_opt.or(val_opt.clone().map(|v| format!("{v:#?}")));
    ComponentOutput::from(val_opt, interface, component_response_to_agent_opt)
}

impl Debug for ComponentRuntimeEngine {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#?}")
    }
}

impl Debug for ComponentRuntimeInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#?}")
    }
}
