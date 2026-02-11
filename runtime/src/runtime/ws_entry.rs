//! Host entry points for the asterai WebSocket interface.
use crate::component::wit::ComponentInterface;
use crate::runtime::env::HostEnv;
use crate::runtime::ws::{WsConfig, WsManager};
use std::future::Future;
use std::sync::Arc;
use wasmtime::StoreContextMut;
use wasmtime::component::Linker;

type HostFuture<'a, T> = Box<dyn Future<Output = Result<T, wasmtime::Error>> + Send + 'a>;

pub fn add_asterai_ws_to_linker(linker: &mut Linker<HostEnv>) -> eyre::Result<()> {
    let mut instance = linker
        .instance("asterai:host-ws/connection@0.1.0")
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("connect", ws_connect)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("send", ws_send)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("close", ws_close)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    Ok(())
}

fn ws_connect(
    mut store: StoreContextMut<HostEnv>,
    (config,): (WsConfig,),
) -> HostFuture<(Result<u64, String>,)> {
    Box::new(async move {
        let result = ws_connect_inner(&mut store, config).await;
        Ok((result,))
    })
}

async fn ws_connect_inner(
    store: &mut StoreContextMut<'_, HostEnv>,
    config: WsConfig,
) -> Result<u64, String> {
    let runtime_data = store
        .data()
        .runtime_data
        .as_ref()
        .ok_or("runtime not initialized")?;
    let ws_manager: Arc<WsManager> = runtime_data
        .ws_manager
        .as_ref()
        .ok_or("ws manager not available")?
        .clone();
    // Identify the calling component.
    let owner = runtime_data
        .last_component
        .lock()
        .unwrap()
        .clone()
        .ok_or("no calling component")?;
    // Look up the component binary for the owner.
    let binary = runtime_data
        .compiled_components
        .iter()
        .find(|(b, _)| b.component().id() == owner.id())
        .map(|(b, _)| b)
        .ok_or_else(|| format!("component '{}' binary not found", owner.id()))?;
    // Validate the component exports incoming-handler.
    let has_export = binary
        .exported_interfaces()
        .iter()
        .any(|e| e.name.starts_with("asterai:host-ws/incoming-handler"));
    if !has_export {
        return Err(format!(
            "component '{}' does not export \
             asterai:host-ws/incoming-handler@0.1.0",
            owner.id()
        ));
    }
    ws_manager.connect(config, binary.clone()).await
}

fn ws_send<'a>(
    store: StoreContextMut<'a, HostEnv>,
    (id, data): (u64, Vec<u8>),
) -> HostFuture<'a, (Result<(), String>,)> {
    Box::new(async move {
        let mgr = match get_ws_manager(&store) {
            Ok(m) => m,
            Err(e) => return Ok((Err(e),)),
        };
        let result = mgr.send(id, data).await;
        Ok((result,))
    })
}

fn ws_close<'a>(store: StoreContextMut<'a, HostEnv>, (id,): (u64,)) -> HostFuture<'a, ()> {
    Box::new(async move {
        if let Ok(mgr) = get_ws_manager(&store) {
            mgr.close(id).await;
        }
        Ok(())
    })
}

fn get_ws_manager(store: &StoreContextMut<HostEnv>) -> Result<Arc<WsManager>, String> {
    store
        .data()
        .runtime_data
        .as_ref()
        .and_then(|rd| rd.ws_manager.clone())
        .ok_or_else(|| "ws manager not available".to_owned())
}

/// Registers sync WS stubs for the sync engine used by dynamic calls.
/// WS operations are not supported in the dynamic call context.
pub fn add_asterai_ws_to_sync_linker(linker: &mut Linker<HostEnv>) -> eyre::Result<()> {
    let mut instance = linker
        .instance("asterai:host-ws/connection@0.1.0")
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap(
            "connect",
            |_store: StoreContextMut<HostEnv>, (_config,): (WsConfig,)| {
                Ok((Err::<u64, String>(
                    "ws not available in dynamic call context".to_owned(),
                ),))
            },
        )
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap(
            "send",
            |_store: StoreContextMut<HostEnv>, (_id, _data): (u64, Vec<u8>)| {
                Ok((Err::<(), String>(
                    "ws not available in dynamic call context".to_owned(),
                ),))
            },
        )
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap(
            "close",
            |_store: StoreContextMut<HostEnv>, (_id,): (u64,)| Ok(()),
        )
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    Ok(())
}
