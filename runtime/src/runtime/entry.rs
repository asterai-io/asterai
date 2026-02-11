//! Host entry points for the asterai host API.
use crate::component::ComponentId;
use crate::component::binary::{ComponentBinary, WasmtimeComponent};
use crate::component::function_interface::ComponentFunctionInterface;
use crate::component::function_name::ComponentFunctionName;
use crate::component::wit::ComponentInterface;
use crate::runtime::env::{HostEnv, HostEnvRuntimeData, create_fresh_store, create_sync_linker};
use crate::runtime::link_components::{register_component_stubs_sync, resolve_component_stubs};
use crate::runtime::parsing::{ValExt, json_value_to_val_typedef};
use crate::runtime::wasm_instance::SYNC_ENGINE;
use crate::runtime::wit_bindings::exports::asterai::host::api::{
    CallError, CallErrorKind, ComponentInfo, FunctionInfo, ParamInfo, RuntimeInfo, TypeInfo,
};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::str::FromStr;
use wasmtime::StoreContextMut;
use wasmtime::component::{Linker, Val};

type HostFuture<'a, T> = Box<dyn Future<Output = Result<T, wasmtime::Error>> + Send + 'a>;

/// Registers sync versions of asterai host functions for the sync
/// engine used by dynamic calls.
pub fn add_asterai_host_to_sync_linker(linker: &mut Linker<HostEnv>) -> eyre::Result<()> {
    let mut instance = linker
        .instance("asterai:host/api@1.0.0")
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap("get-runtime-info", get_runtime_info_sync)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap("list-components", list_components_sync)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap("list-other-components", list_other_components_sync)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap("get-component", get_component_sync)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap("component-implements", component_implements_sync)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap("call-component-function", call_component_function_sync)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    Ok(())
}

pub fn add_asterai_host_to_linker(linker: &mut Linker<HostEnv>) -> eyre::Result<()> {
    let mut instance = linker
        .instance("asterai:host/api@1.0.0")
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("get-runtime-info", get_runtime_info)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("list-components", list_components)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("list-other-components", list_other_components)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("get-component", get_component)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("component-implements", component_implements)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("call-component-function", call_component_function)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    Ok(())
}

fn get_runtime_info<'a>(
    _store: StoreContextMut<'a, HostEnv>,
    _params: (),
) -> HostFuture<'a, (RuntimeInfo,)> {
    Box::new(async move {
        let runtime_info = RuntimeInfo {
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        Ok((runtime_info,))
    })
}

fn list_components<'a>(
    store: StoreContextMut<'a, HostEnv>,
    _params: (),
) -> HostFuture<'a, (Vec<ComponentInfo>,)> {
    Box::new(async move {
        let infos = build_all_component_infos(&store);
        Ok((infos,))
    })
}

fn list_other_components<'a>(
    store: StoreContextMut<'a, HostEnv>,
    _params: (),
) -> HostFuture<'a, (Vec<ComponentInfo>,)> {
    Box::new(async move {
        let caller_id = get_last_component_id(&store);
        let infos = build_all_component_infos(&store)
            .into_iter()
            .filter(|info| Some(&info.name) != caller_id.as_ref())
            .collect();
        Ok((infos,))
    })
}

fn get_component<'a>(
    store: StoreContextMut<'a, HostEnv>,
    (name,): (String,),
) -> HostFuture<'a, (Option<ComponentInfo>,)> {
    Box::new(async move {
        let info = build_all_component_infos(&store)
            .into_iter()
            .find(|info| info.name == name);
        Ok((info,))
    })
}

fn component_implements<'a>(
    store: StoreContextMut<'a, HostEnv>,
    (component_name, interface_name): (String, String),
) -> HostFuture<'a, (bool,)> {
    Box::new(async move {
        let found = build_all_component_infos(&store)
            .into_iter()
            .find(|info| info.name == component_name)
            .is_some_and(|info| info.interfaces.contains(&interface_name));
        Ok((found,))
    })
}

fn call_component_function<'a>(
    mut store: StoreContextMut<'a, HostEnv>,
    (component_name, function_name_str, args_json): (String, String, String),
) -> HostFuture<'a, (Result<String, CallError>,)> {
    Box::new(async move {
        let result = call_component_function_inner(
            &mut store,
            &component_name,
            &function_name_str,
            &args_json,
        )
        .await;
        Ok((result,))
    })
}

async fn call_component_function_inner(
    store: &mut StoreContextMut<'_, HostEnv>,
    component_name: &str,
    function_name_str: &str,
    args_json: &str,
) -> Result<String, CallError> {
    let comp_id = ComponentId::from_str(component_name).map_err(|e| CallError {
        kind: CallErrorKind::ComponentNotFound,
        message: format!("invalid component name: {e}"),
    })?;
    let function_name = ComponentFunctionName::from_str(function_name_str).unwrap();
    // Extract everything we need from the caller's store.
    let (compiled_components, resolve, function, env_vars, preopened_dirs, runtime_data) = {
        let runtime_data = store.data().runtime_data.as_ref().ok_or(CallError {
            kind: CallErrorKind::InvocationFailed,
            message: "runtime not initialized".to_owned(),
        })?;
        let (binary, _) = runtime_data
            .compiled_components
            .iter()
            .find(|(cb, _)| cb.component().id() == comp_id)
            .ok_or(CallError {
                kind: CallErrorKind::ComponentNotFound,
                message: format!("component '{component_name}' not found"),
            })?;
        let func = binary
            .get_functions()
            .into_iter()
            .find(|f| f.name == function_name)
            .ok_or(CallError {
                kind: CallErrorKind::FunctionNotFound,
                message: format!(
                    "function '{function_name_str}' not found \
                     on '{component_name}'"
                ),
            })?;
        (
            runtime_data.compiled_components.clone(),
            binary.wit().resolve().clone(),
            func,
            runtime_data.env_vars.clone(),
            runtime_data.preopened_dirs.clone(),
            runtime_data.clone(),
        )
    };
    // Parse and validate JSON args.
    let json_args: Vec<serde_json::Value> =
        serde_json::from_str(args_json).map_err(|e| CallError {
            kind: CallErrorKind::InvalidArgs,
            message: format!("invalid JSON args: {e}"),
        })?;
    if json_args.len() != function.inputs.len() {
        return Err(CallError {
            kind: CallErrorKind::InvalidArgs,
            message: format!(
                "expected {} arg(s), got {}",
                function.inputs.len(),
                json_args.len()
            ),
        });
    }
    let inputs: Vec<Val> = json_args
        .iter()
        .zip(function.inputs.iter())
        .map(|(arg, (_name, type_def))| json_value_to_val_typedef(arg, type_def, &resolve))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CallError {
            kind: CallErrorKind::InvalidArgs,
            message: format!("failed to convert args: {e}"),
        })?;
    // Run on a blocking thread with a sync engine to avoid the nested
    // `run_concurrent` assertion. The sync engine's `Func::call` bypasses
    // wasmtime's concurrent module entirely, so forwarding stubs can
    // safely call other components without reentrancy issues.
    tokio::task::spawn_blocking(move || {
        execute_dynamic_call(
            compiled_components,
            comp_id,
            function,
            inputs,
            env_vars,
            preopened_dirs,
            runtime_data,
        )
    })
    .await
    .map_err(|e| CallError {
        kind: CallErrorKind::InvocationFailed,
        message: format!("{e}"),
    })?
}

/// Runs on a blocking thread with a sync engine.
fn execute_dynamic_call(
    compiled_components: Vec<(ComponentBinary, WasmtimeComponent)>,
    comp_id: ComponentId,
    function: ComponentFunctionInterface,
    inputs: Vec<Val>,
    env_vars: HashMap<String, String>,
    preopened_dirs: Vec<PathBuf>,
    runtime_data: HostEnvRuntimeData,
) -> Result<String, CallError> {
    let engine = &*SYNC_ENGINE;
    let mut store = create_fresh_store(engine, &env_vars, &preopened_dirs);
    store.data_mut().runtime_data = Some(runtime_data);
    let mut linker = create_sync_linker(engine).map_err(|e| CallError {
        kind: CallErrorKind::InvocationFailed,
        message: format!("failed to set up linker: {e}"),
    })?;
    let binaries: Vec<_> = compiled_components.iter().map(|(b, _)| b.clone()).collect();
    let stubs = register_component_stubs_sync(&binaries, &mut linker).map_err(|e| CallError {
        kind: CallErrorKind::InvocationFailed,
        message: format!("failed to register stubs: {e}"),
    })?;
    let mut all_instances = Vec::new();
    let mut target_instance = None;
    for (binary, _) in &compiled_components {
        let compiled = binary
            .compile_for_engine_sync(engine)
            .map_err(|e| CallError {
                kind: CallErrorKind::InvocationFailed,
                message: format!("failed to compile '{}': {e}", binary.component().id()),
            })?;
        let instance = linker
            .instantiate(&mut store, &compiled)
            .map_err(|e| CallError {
                kind: CallErrorKind::InvocationFailed,
                message: format!(
                    "failed to instantiate component '{}': {e}",
                    binary.component().id()
                ),
            })?;
        resolve_component_stubs(binary, &instance, &mut store, &stubs).map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("failed to resolve stubs: {e}"),
        })?;
        if binary.component().id() == comp_id {
            target_instance = Some(instance);
        }
        all_instances.push((binary.clone(), instance));
    }
    // Store instances so nested call-component-function calls can find them.
    store.data_mut().sync_instances = all_instances;
    let instance = target_instance.ok_or(CallError {
        kind: CallErrorKind::ComponentNotFound,
        message: format!("component '{}' not found", comp_id),
    })?;
    let func = function
        .get_func(&mut store, &instance)
        .map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("failed to get function: {e}"),
        })?;
    let mut results = function.new_results_vec();
    func.call(&mut store, &inputs, &mut results)
        .map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("{e:#}"),
        })?;
    func.post_return(&mut store).map_err(|e| CallError {
        kind: CallErrorKind::InvocationFailed,
        message: format!("{e:#}"),
    })?;
    let output_val = results.into_iter().next();
    let json_output = output_val
        .and_then(|v| v.try_into_json_value())
        .unwrap_or(serde_json::Value::Null);
    serde_json::to_string(&json_output).map_err(|e| CallError {
        kind: CallErrorKind::SerializationFailed,
        message: format!("{e}"),
    })
}

fn get_runtime_info_sync(
    _store: StoreContextMut<HostEnv>,
    _params: (),
) -> wasmtime::Result<(RuntimeInfo,)> {
    let info = RuntimeInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    Ok((info,))
}

fn list_components_sync(
    store: StoreContextMut<HostEnv>,
    _params: (),
) -> wasmtime::Result<(Vec<ComponentInfo>,)> {
    Ok((build_all_component_infos(&store),))
}

fn list_other_components_sync(
    store: StoreContextMut<HostEnv>,
    _params: (),
) -> wasmtime::Result<(Vec<ComponentInfo>,)> {
    let caller_id = get_last_component_id(&store);
    let infos = build_all_component_infos(&store)
        .into_iter()
        .filter(|info| Some(&info.name) != caller_id.as_ref())
        .collect();
    Ok((infos,))
}

fn get_component_sync(
    store: StoreContextMut<HostEnv>,
    (name,): (String,),
) -> wasmtime::Result<(Option<ComponentInfo>,)> {
    let info = build_all_component_infos(&store)
        .into_iter()
        .find(|info| info.name == name);
    Ok((info,))
}

fn component_implements_sync(
    store: StoreContextMut<HostEnv>,
    (component_name, interface_name): (String, String),
) -> wasmtime::Result<(bool,)> {
    let found = build_all_component_infos(&store)
        .into_iter()
        .find(|info| info.name == component_name)
        .is_some_and(|info| info.interfaces.contains(&interface_name));
    Ok((found,))
}

fn call_component_function_sync(
    mut store: StoreContextMut<HostEnv>,
    (component_name, function_name_str, args_json): (String, String, String),
) -> wasmtime::Result<(Result<String, CallError>,)> {
    let result = call_component_function_sync_inner(
        &mut store,
        &component_name,
        &function_name_str,
        &args_json,
    );
    Ok((result,))
}

/// Sync implementation of `call-component-function` for the sync engine.
/// Uses `Func::call` which is naturally reentrant in sync mode.
fn call_component_function_sync_inner(
    store: &mut StoreContextMut<'_, HostEnv>,
    component_name: &str,
    function_name_str: &str,
    args_json: &str,
) -> Result<String, CallError> {
    let comp_id = ComponentId::from_str(component_name).map_err(|e| CallError {
        kind: CallErrorKind::ComponentNotFound,
        message: format!("invalid component name: {e}"),
    })?;
    let function_name = ComponentFunctionName::from_str(function_name_str).unwrap();
    // Find the instance and function from the sync store.
    let (binary, instance) = store
        .data()
        .sync_instances
        .iter()
        .find(|(b, _)| b.component().id() == comp_id)
        .ok_or(CallError {
            kind: CallErrorKind::ComponentNotFound,
            message: format!("component '{component_name}' not found"),
        })?
        .clone();
    let resolve = binary.wit().resolve().clone();
    let function = binary
        .get_functions()
        .into_iter()
        .find(|f| f.name == function_name)
        .ok_or(CallError {
            kind: CallErrorKind::FunctionNotFound,
            message: format!(
                "function '{function_name_str}' not found \
                 on '{component_name}'"
            ),
        })?;
    // Parse and validate JSON args.
    let json_args: Vec<serde_json::Value> =
        serde_json::from_str(args_json).map_err(|e| CallError {
            kind: CallErrorKind::InvalidArgs,
            message: format!("invalid JSON args: {e}"),
        })?;
    if json_args.len() != function.inputs.len() {
        return Err(CallError {
            kind: CallErrorKind::InvalidArgs,
            message: format!(
                "expected {} arg(s), got {}",
                function.inputs.len(),
                json_args.len()
            ),
        });
    }
    let inputs: Vec<Val> = json_args
        .iter()
        .zip(function.inputs.iter())
        .map(|(arg, (_name, type_def))| json_value_to_val_typedef(arg, type_def, &resolve))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CallError {
            kind: CallErrorKind::InvalidArgs,
            message: format!("failed to convert args: {e}"),
        })?;
    let func = function
        .get_func(&mut *store, &instance)
        .map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("failed to get function: {e}"),
        })?;
    let mut results = function.new_results_vec();
    func.call(&mut *store, &inputs, &mut results)
        .map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("{e:#}"),
        })?;
    func.post_return(&mut *store).map_err(|e| CallError {
        kind: CallErrorKind::InvocationFailed,
        message: format!("{e:#}"),
    })?;
    let output_val = results.into_iter().next();
    let json_output = output_val
        .and_then(|v| v.try_into_json_value())
        .unwrap_or(serde_json::Value::Null);
    serde_json::to_string(&json_output).map_err(|e| CallError {
        kind: CallErrorKind::SerializationFailed,
        message: format!("{e}"),
    })
}

fn build_all_component_infos(store: &StoreContextMut<HostEnv>) -> Vec<ComponentInfo> {
    let Some(runtime_data) = store.data().runtime_data.as_ref() else {
        return Vec::new();
    };
    runtime_data
        .instances
        .iter()
        .map(|instance| {
            let component = instance.component_interface.component();
            let exported = instance.component_interface.exported_interfaces();
            let interfaces: Vec<String> = exported
                .iter()
                .map(|e| e.name.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let description = build_component_description(
                instance.component_interface.wit().world_docs(),
                &exported,
            );
            let functions = exported
                .iter()
                .flat_map(|iface| {
                    let short_iface_name = extract_short_interface_name(&iface.name);
                    iface.functions.iter().map(move |f| FunctionInfo {
                        name: f.name.clone(),
                        interface_name: Some(short_iface_name.clone()),
                        description: f.docs.clone(),
                        inputs: f
                            .params
                            .iter()
                            .map(|p| ParamInfo {
                                name: p.name.clone(),
                                type_name: p.type_name.clone(),
                                type_schema: p.type_schema.clone(),
                            })
                            .collect(),
                        output: f.return_type_name.as_ref().map(|name| TypeInfo {
                            type_name: name.clone(),
                            type_schema: f.return_type_schema.clone().unwrap_or_default(),
                        }),
                    })
                })
                .collect();
            ComponentInfo {
                name: component.id().to_string(),
                version: component.version().to_string(),
                interfaces,
                description,
                functions,
            }
        })
        .collect()
}

fn build_component_description(
    world_docs: Option<String>,
    exported: &[crate::component::wit::ExportedInterface],
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(docs) = &world_docs {
        parts.push(docs.trim().to_owned());
    }
    for iface in exported {
        if let Some(docs) = &iface.docs {
            let name = extract_short_interface_name(&iface.name);
            parts.push(format!("{name}: {}", docs.trim()));
        }
    }
    match parts.is_empty() {
        true => None,
        false => Some(parts.join("\n\n")),
    }
}

/// Extract the short interface name from a fully qualified name.
/// e.g. "asterai:llm/llm@1.0.0" => "llm".
fn extract_short_interface_name(fq_name: &str) -> String {
    let name = fq_name.rsplit_once('/').map(|(_, n)| n).unwrap_or(fq_name);
    name.split_once('@')
        .map(|(n, _)| n)
        .unwrap_or(name)
        .to_owned()
}

fn get_last_component_id(store: &StoreContextMut<HostEnv>) -> Option<String> {
    store
        .data()
        .runtime_data
        .as_ref()?
        .last_component
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.id().to_string())
}
