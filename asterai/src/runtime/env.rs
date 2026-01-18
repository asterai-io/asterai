use crate::component::Component;
use crate::runtime::output::ComponentOutput;
use crate::runtime::wasm_instance::ComponentRuntimeInstance;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

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
