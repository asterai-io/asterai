//! Host entry points for the asterai host API.
use crate::component::wit::ComponentInterface;
use crate::runtime::env::HostEnv;
use crate::runtime::wit_bindings::exports::asterai::host::api::{
    CallError, CallErrorKind, ComponentInfo, RuntimeInfo,
};
use std::collections::HashSet;
use std::future::Future;
use wasmtime::StoreContextMut;
use wasmtime::component::Linker;

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
) -> Box<dyn Future<Output = Result<(RuntimeInfo,), wasmtime::Error>> + Send + 'a> {
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
) -> Box<dyn Future<Output = Result<(Vec<ComponentInfo>,), wasmtime::Error>> + Send + 'a> {
    Box::new(async move {
        let infos = build_all_component_infos(&store);
        Ok((infos,))
    })
}

fn list_other_components<'a>(
    store: StoreContextMut<'a, HostEnv>,
    _params: (),
) -> Box<dyn Future<Output = Result<(Vec<ComponentInfo>,), wasmtime::Error>> + Send + 'a> {
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
) -> Box<dyn Future<Output = Result<(Option<ComponentInfo>,), wasmtime::Error>> + Send + 'a> {
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
) -> Box<dyn Future<Output = Result<(bool,), wasmtime::Error>> + Send + 'a> {
    Box::new(async move {
        let found = build_all_component_infos(&store)
            .into_iter()
            .find(|info| info.name == component_name)
            .is_some_and(|info| info.interfaces.contains(&interface_name));
        Ok((found,))
    })
}

fn call_component_function<'a>(
    _store: StoreContextMut<'a, HostEnv>,
    (_component_name, _function_name, _args_json): (String, String, String),
) -> Box<dyn Future<Output = Result<(Result<String, CallError>,), wasmtime::Error>> + Send + 'a> {
    Box::new(async move {
        Ok((Err(CallError {
            kind: CallErrorKind::InvocationFailed,
            message: "not yet implemented".to_owned(),
        }),))
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
            let interfaces: Vec<String> = instance
                .component_interface
                .exported_interfaces()
                .into_iter()
                .map(|e| e.name)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            ComponentInfo {
                name: component.id().to_string(),
                version: component.version().to_string(),
                interfaces,
            }
        })
        .collect()
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
