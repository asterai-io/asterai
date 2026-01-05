use crate::plugin::Plugin;
use crate::runtime::output::PluginOutput;
use crate::runtime::wasm_instance::PluginRuntimeInstance;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

/// The plugin host env data.
/// Data held here is accessible as a function argument
/// when handling host functions called by plugins, for
/// accessing host resources when handling a plugin request.
pub struct HostEnv {
    pub table: ResourceTable,
    pub wasi_ctx: WasiCtx,
    pub http_ctx: WasiHttpCtx,
    pub runtime_data: Option<HostEnvRuntimeData>,
    pub plugin_output_tx: mpsc::Sender<PluginOutput>,
}

#[derive(Clone)]
pub struct HostEnvRuntimeData {
    pub app_id: Uuid,
    pub instances: Vec<PluginRuntimeInstance>,
    /// The last plugin that was called.
    /// Plugins need to only be able to access their own storage,
    /// so this needs to be implemented correctly for security purposes.
    pub last_plugin: Arc<Mutex<Option<Plugin>>>,
    pub plugin_response_to_agent: Option<String>,
}

impl WasiView for HostEnv {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
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
