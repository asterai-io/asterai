use crate::plugin::Plugin;
use crate::plugin::function_name::PluginFunctionName;
use crate::plugin::interface::{PluginFunctionInterface, PluginInterface};
use crate::runtime::entry::add_asterai_host_to_linker;
use crate::runtime::env::{HostEnv, HostEnvRuntimeData};
use crate::runtime::output::PluginOutput;
use crate::runtime::std_out_err::{PluginStderr, PluginStdout};
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
use wasmtime_wasi_http::WasiHttpCtx;

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.async_support(true);
    Engine::new(&config).unwrap()
});

pub struct PluginRuntimeEngine {
    store: Store<StoreState>,
    instances: Vec<PluginRuntimeInstance>,
}

#[derive(Clone)]
pub struct PluginRuntimeInstance {
    // TODO add app_plugin_id here to make it easily accessible
    // in host entry functions.
    pub plugin_interface: PluginInterface,
    pub app_id: Uuid,
    instance: Instance,
}

unsafe impl Send for HostEnv {}

// #[derive(Debug)]
// pub struct StoreState {}
pub type StoreState = HostEnv;

impl PluginRuntimeEngine {
    pub async fn new(
        mut plugins: Vec<PluginInterface>,
        app_id: Uuid,
        asterai_http_api_origin: String,
        plugin_output_tx: mpsc::Sender<PluginOutput>,
    ) -> eyre::Result<Self> {
        let engine = &ENGINE;
        let last_plugin = Arc::new(Mutex::new(None));
        // Create a WASI context and put it in a Store; all instances in the store
        // share this context. `WasiCtxBuilder` provides a number of ways to
        // configure what the target program will have access to.
        let wasi_ctx = WasiCtxBuilder::new()
            .stdout(PluginStdout {
                app_id,
                plugin: last_plugin.clone(),
            })
            .stderr(PluginStderr {
                app_id,
                plugin: last_plugin.clone(),
            })
            .inherit_network()
            .build();
        let http_ctx = WasiHttpCtx::new();
        let table = ResourceTable::new();
        let host_env = HostEnv {
            runtime_data: None,
            wasi_ctx,
            http_ctx,
            table,
            asterai_http_api_origin,
            plugin_output_tx,
        };
        let mut store = Store::new(engine, host_env);
        let mut linker = Linker::new(engine);
        // Prevent "defined twice" errors.
        linker.allow_shadowing(true);
        wasmtime_wasi::add_to_linker_async(&mut linker).map_err(|e| eyre!(e))?;
        wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker).map_err(|e| eyre!(e))?;
        add_asterai_host_to_linker(&mut linker)?;
        let mut instances = Vec::new();
        // Sort by ascending order of imports count,
        // so that components with no dependencies are added first.
        plugins.sort_by(|a, b| b.get_imports_count().cmp(&a.get_imports_count()));
        // TODO fix this up.
        for interface in plugins.into_iter() {
            trace!("@ interface {}", interface.plugin().id());
            trace!("imports count: {}", interface.get_imports_count());
            let component = interface.fetch_compiled_component(engine).await?;
            trace!("fetched compiled component");
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .map_err(|e| eyre!("{e:#?}"))
                .with_context(|| "failed to initiate component")?;
            let instance = PluginRuntimeInstance {
                plugin_interface: interface,
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
            last_plugin,
            plugin_response_to_agent: None,
        });
        Ok(runtime_engine)
    }

    pub fn instances(&self) -> &[PluginRuntimeInstance] {
        &self.instances
    }

    pub async fn call(
        &mut self,
        function_interface: PluginFunctionInterface,
        inputs: &[Val],
    ) -> eyre::Result<Option<PluginOutput>> {
        // This is an uninitialised vec of the results.
        let mut results = function_interface.new_results_vec();
        self.call_raw(&function_interface, &inputs, &mut results)
            .await?;
        let output_opt = parse_plugin_output(self.store.as_context(), results, function_interface);
        Ok(output_opt)
    }

    /// Makes a function call to a WASM component.
    async fn call_raw(
        &mut self,
        function_interface: &PluginFunctionInterface,
        args: &[Val],
        results: &mut [Val],
    ) -> eyre::Result<()> {
        let instance_opt = self.instances.iter().find(|instance| {
            instance.plugin_interface.plugin().id() == function_interface.plugin.id()
        });
        let Some(instance) = instance_opt else {
            return Err(eyre!(
                "instance not found for function '{:#?}'",
                function_interface
            ));
        };
        let functions = instance.plugin_interface.get_functions();
        let function_opt = functions
            .into_iter()
            .find(|f| &f.name == function_interface.name());
        let Some(function) = function_opt else {
            return Err(eyre!("function not found in instance"));
        };
        let func = function.get_func(&mut self.store, &instance.instance)?;
        let plugin = function.plugin.clone();
        call_wasm_component_function(
            &func,
            &function.name,
            self.store.as_context_mut(),
            args,
            results,
            plugin,
        )
        .await?;
        Ok(())
    }
}

impl PluginRuntimeInstance {
    /// Add all plugin exports to the linker.
    pub fn add_to_linker(
        &self,
        linker: &mut Linker<HostEnv>,
        mut store: impl AsContextMut,
    ) -> eyre::Result<()> {
        trace!(
            "adding plugin to linker: {}",
            self.plugin_interface.plugin()
        );
        let functions = self.plugin_interface.get_functions();
        let mut functions_by_instance: HashMap<String, Vec<PluginFunctionInterface>> =
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
                let plugin = self.plugin_interface.plugin().clone();
                let func_name_cloned = function.name.clone();
                let func_name = function.name.clone();
                trace!("adding function to linker (export {instance_name}): '{func_name_cloned}'");
                exported_instance
                    .func_new_async(
                        &func_name_cloned.name,
                        move |mut store, params, mut results| {
                            let plugin = plugin.clone();
                            let func_name = func_name.clone();
                            let function_cloned = function.clone();
                            Box::new(async move {
                                call_wasm_component_function(
                                    &func,
                                    &func_name,
                                    store.as_context_mut(),
                                    params,
                                    &mut results,
                                    plugin,
                                )
                                .await
                                .unwrap();
                                let output_opt = parse_plugin_output(
                                    store.as_context(),
                                    results.to_vec(),
                                    function_cloned,
                                );
                                if let Some(output) = output_opt {
                                    // Forward output to channel.
                                    // This output is not from the directly called function,
                                    // but from an internal function call somewhere in the stack.
                                    store.data().plugin_output_tx.send(output).await.unwrap();
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
    func_name: &PluginFunctionName,
    mut store: StoreContextMut<'a, HostEnv>,
    args: &[Val],
    results: &mut [Val],
    plugin: Plugin,
) -> eyre::Result<()> {
    let plugin_id = plugin.id().clone();
    trace!(
        "calling function '{func_name}' from plugin '{}'",
        plugin.id()
    );
    set_last_plugin(plugin, &mut store);
    func.call_async(&mut store, args, results)
        .await
        .map_err(|e| {
            eyre!(
                "failed to call func '{func_name}' from plugin '{}': {e:#?}",
                plugin_id,
            )
        })?;
    func.post_return_async(&mut store)
        .await
        .map_err(|e| eyre!("{e:#?}"))?;
    Ok(())
}

/// Set this plugin as the last one called.
/// This is necessary for knowing the plugin ID in case the function
/// called accesses host functions such as logging or any other part of the host API.
fn set_last_plugin(plugin: Plugin, store: &mut StoreContextMut<HostEnv>) {
    *store
        .data_mut()
        .runtime_data
        .as_mut()
        .unwrap()
        .last_plugin
        .lock()
        .unwrap() = Some(plugin);
}

fn parse_plugin_output(
    store: StoreContext<HostEnv>,
    results: Vec<Val>,
    interface: PluginFunctionInterface,
) -> Option<PluginOutput> {
    let val_opt = results.into_iter().next();
    let plugin_response_to_agent_opt = store
        .data()
        .runtime_data
        .as_ref()
        .and_then(|r| r.plugin_response_to_agent.clone());
    // If the app is an agent, then the return value of functions will be serialized to text
    // and sent to the agent unless the function calls the API method `send_response_to_agent`
    // which can be used to override the return value.
    //
    // This overriding by `send_response_to_agent` can be useful if the function returns a large
    // output that the agent does not need (such as to be consumed by the client/front-end),
    // allowing the plugin to re-word the output as natural language while keeping the structured
    // output usable for the client to consume it.
    //
    // The input to `send_response_to_agent` is represented by `plugin_response_to_agent_opt` here.
    // TODO: add API method for `send_no_response_to_agent` to prevent any response from being sent.
    // TODO: revisit above if app is not an agent (e.g. blueprint app)
    let plugin_response_to_agent_opt =
        plugin_response_to_agent_opt.or(val_opt.clone().map(|v| format!("{v:#?}")));
    PluginOutput::from(val_opt, interface, plugin_response_to_agent_opt)
}

impl Debug for PluginRuntimeEngine {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#?}")
    }
}

impl Debug for PluginRuntimeInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#?}")
    }
}
