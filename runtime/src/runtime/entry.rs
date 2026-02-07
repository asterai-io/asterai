//! Host entry points for the asterai host API.
use crate::component::ComponentId;
use crate::component::function_name::ComponentFunctionName;
use crate::component::wit::ComponentInterface;
use crate::runtime::env::{HostEnv, create_fresh_store, create_linker};
use crate::runtime::parsing::{ValExt, json_value_to_val_typedef};
use crate::runtime::wasm_instance::ENGINE;
use crate::runtime::wit_bindings::exports::asterai::host::api::{
    CallError, CallErrorKind, ComponentInfo, FunctionInfo, ParamInfo, RuntimeInfo,
};
use std::collections::HashSet;
use std::future::Future;
use std::str::FromStr;
use wasmtime::StoreContextMut;
use wasmtime::component::{Linker, Val};

type HostFuture<'a, T> = Box<dyn Future<Output = Result<T, wasmtime::Error>> + Send + 'a>;

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
    // Extract compiled component, function info, and env vars from caller's store.
    let (compiled_component, _component_binary, function, env_vars) = {
        let runtime_data = store.data().runtime_data.as_ref().ok_or(CallError {
            kind: CallErrorKind::InvocationFailed,
            message: "runtime not initialized".to_owned(),
        })?;
        let (binary, component) = runtime_data
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
                message: format!("function '{function_name_str}' not found on '{component_name}'"),
            })?;
        (
            component.clone(),
            binary.clone(),
            func,
            runtime_data.env_vars.clone(),
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
        .map(|(arg, (_name, type_def))| json_value_to_val_typedef(arg, type_def))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CallError {
            kind: CallErrorKind::InvalidArgs,
            message: format!("failed to convert args: {e}"),
        })?;
    // Create a fresh store to avoid reentrant call issues.
    let engine = &*ENGINE;
    let mut fresh_store = create_fresh_store(engine, &env_vars);
    let linker = create_linker(engine).map_err(|e| CallError {
        kind: CallErrorKind::InvocationFailed,
        message: format!("failed to set up linker: {e}"),
    })?;
    let instance = linker
        .instantiate_async(&mut fresh_store, &compiled_component)
        .await
        .map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("failed to instantiate component: {e}"),
        })?;
    // Call the function on the fresh instance.
    let mut results = function.new_results_vec();
    let func = function
        .get_func(&mut fresh_store, &instance)
        .map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("failed to get function: {e}"),
        })?;
    func.call_async(&mut fresh_store, &inputs, &mut results)
        .await
        .map_err(|e| CallError {
            kind: CallErrorKind::InvocationFailed,
            message: format!("{e:#}"),
        })?;
    func.post_return_async(&mut fresh_store)
        .await
        .map_err(|e| CallError {
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
                    iface.functions.iter().map(|f| FunctionInfo {
                        name: f.name.clone(),
                        description: f.docs.clone(),
                        inputs: f
                            .params
                            .iter()
                            .map(|p| ParamInfo {
                                name: p.name.clone(),
                                type_description: p.type_name.clone(),
                            })
                            .collect(),
                        output: f.return_type.clone(),
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
            let name = iface
                .name
                .rsplit_once('/')
                .map(|(_, n)| n)
                .unwrap_or(&iface.name);
            // Strip version suffix if present.
            let name = name.split_once('@').map(|(n, _)| n).unwrap_or(name);
            parts.push(format!("{name}: {}", docs.trim()));
        }
    }
    match parts.is_empty() {
        true => None,
        false => Some(parts.join("\n\n")),
    }
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
