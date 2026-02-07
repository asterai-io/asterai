use crate::component::Component;
use crate::component::binary::{ComponentBinary, WasmtimeComponent};
use crate::runtime::entry::add_asterai_host_to_linker;
use crate::runtime::output::ComponentOutput;
use crate::runtime::std_out_err::{ComponentStderr, ComponentStdout};
use crate::runtime::wasm_instance::ComponentRuntimeInstance;
use eyre::eyre;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;
use wasmtime::component::{Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::p2::add_to_linker_async;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView, add_only_http_to_linker_async};

/// The component host env data.
/// Data held here is accessible as a function argument
/// when handling host functions called by components, for
/// accessing host resources when handling a component request.
pub struct HostEnv {
    pub table: ResourceTable,
    pub wasi_ctx: WasiCtx,
    pub http_ctx: WasiHttpCtx,
    pub runtime_data: Option<HostEnvRuntimeData>,
    pub component_output_tx: mpsc::Sender<ComponentOutput>,
}

#[derive(Clone)]
pub struct HostEnvRuntimeData {
    pub app_id: Uuid,
    pub instances: Vec<ComponentRuntimeInstance>,
    /// The last component that was called.
    /// Plugins need to only be able to access their own storage,
    /// so this needs to be implemented correctly for security purposes.
    pub last_component: Arc<Mutex<Option<Component>>>,
    pub component_response_to_agent: Option<String>,
    /// Pre-compiled components for dynamic calls (fresh store per call).
    pub compiled_components: Vec<(ComponentBinary, WasmtimeComponent)>,
    /// Environment variables to inject into fresh stores for dynamic calls.
    pub env_vars: HashMap<String, String>,
}

/// Create a Store with an externally provided app ID and output channel.
pub fn create_store(
    engine: &Engine,
    env_vars: &HashMap<String, String>,
    app_id: Uuid,
    component_output_tx: mpsc::Sender<ComponentOutput>,
) -> Store<HostEnv> {
    let mut wasi_ctx = WasiCtxBuilder::new();
    wasi_ctx
        .stdout(ComponentStdout { app_id })
        .stderr(ComponentStderr { app_id })
        .inherit_network();
    for (key, value) in env_vars {
        wasi_ctx.env(key, value);
    }
    let host_env = HostEnv {
        runtime_data: None,
        wasi_ctx: wasi_ctx.build(),
        http_ctx: WasiHttpCtx::new(),
        table: ResourceTable::new(),
        component_output_tx,
    };
    Store::new(engine, host_env)
}

/// Create a disposable Store with a new app ID and a drain output channel.
pub fn create_fresh_store(engine: &Engine, env_vars: &HashMap<String, String>) -> Store<HostEnv> {
    let (tx, mut rx) = mpsc::channel(32);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    create_store(engine, env_vars, Uuid::new_v4(), tx)
}

/// Create a Linker with WASI, HTTP, and asterai host bindings.
pub fn create_linker(engine: &Engine) -> eyre::Result<Linker<HostEnv>> {
    let mut linker = Linker::new(engine);
    linker.allow_shadowing(true);
    add_to_linker_async(&mut linker).map_err(|e| eyre!("{e}"))?;
    add_only_http_to_linker_async(&mut linker).map_err(|e| eyre!("{e}"))?;
    add_asterai_host_to_linker(&mut linker)?;
    Ok(linker)
}

impl WasiView for HostEnv {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for HostEnv {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http_ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}
