use crate::component::binary::ComponentBinary;
use crate::runtime::wasm_instance::SharedStore;
use eyre::eyre;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use log::{error, info, trace, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::net::TcpStream;
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;
use wasmtime::component::{ComponentNamedList, Lower, TypedFunc};

pub type ConnectionId = u64;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = SplitSink<WsStream, Message>;
type WsSource = SplitStream<WsStream>;

#[derive(wasmtime::component::ComponentType, wasmtime::component::Lift)]
#[component(record)]
pub struct WsConfig {
    pub url: String,
    pub headers: Vec<(String, String)>,
    #[component(name = "auto-reconnect")]
    pub auto_reconnect: bool,
}

struct WsConnection {
    write_tx: mpsc::Sender<Message>,
    cancel_token: CancellationToken,
}

/// Manages WebSocket connections for WASM components.
///
/// The store is set after construction via [`set_store`](Self::set_store)
/// because of a circular dependency: WsManager is referenced by the
/// store's runtime data, so the store cannot exist before the manager.
pub struct WsManager {
    connections: RwLock<HashMap<ConnectionId, WsConnection>>,
    next_id: AtomicU64,
    store: OnceLock<SharedStore>,
}

impl WsManager {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            store: OnceLock::new(),
        }
    }

    pub fn set_store(&self, store: SharedStore) {
        self.store.set(store).ok();
    }

    pub fn shared_store(&self) -> Option<&SharedStore> {
        self.store.get()
    }

    pub async fn connect(
        self: &Arc<Self>,
        config: WsConfig,
        owner_binary: ComponentBinary,
    ) -> Result<ConnectionId, String> {
        let conn_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let config = Arc::new(config);
        let stream = open_ws_connection(&config).await?;
        let (sink, source) = stream.split();
        let cancel_token = CancellationToken::new();
        let (write_tx, write_rx) = mpsc::channel::<Message>(64);
        tokio::spawn(write_loop(sink, write_rx));
        let manager = Arc::clone(self);
        let read_config = Arc::clone(&config);
        let read_binary = owner_binary.clone();
        let read_cancel = cancel_token.clone();
        tokio::spawn(async move {
            read_loop(
                source,
                conn_id,
                read_config,
                read_binary,
                read_cancel,
                manager,
            )
            .await;
        });
        let connection = WsConnection {
            write_tx,
            cancel_token,
        };
        self.connections.write().await.insert(conn_id, connection);
        info!("ws connection {conn_id} opened");
        Ok(conn_id)
    }

    pub async fn send(&self, id: ConnectionId, data: Vec<u8>) -> Result<(), String> {
        let connections = self.connections.read().await;
        let conn = connections
            .get(&id)
            .ok_or_else(|| format!("connection {id} not found"))?;
        conn.write_tx
            .send(Message::Binary(data.into()))
            .await
            .map_err(|e| format!("send failed: {e}"))
    }

    pub async fn close(&self, id: ConnectionId) {
        let conn = self.connections.write().await.remove(&id);
        if let Some(conn) = conn {
            conn.cancel_token.cancel();
            // Send a close frame best-effort.
            let _ = conn.write_tx.send(Message::Close(None)).await;
            drop(conn.write_tx);
            info!("ws connection {id} closed");
        }
    }

    pub async fn close_all(&self) {
        let ids: Vec<ConnectionId> = self.connections.read().await.keys().copied().collect();
        for id in ids {
            self.close(id).await;
        }
    }

    /// Replace the write channel for a reconnected connection.
    async fn replace_writer(&self, id: ConnectionId, new_tx: mpsc::Sender<Message>) {
        let mut connections = self.connections.write().await;
        if let Some(conn) = connections.get_mut(&id) {
            conn.write_tx = new_tx;
        }
    }
}

async fn open_ws_connection(config: &WsConfig) -> Result<WsStream, String> {
    let mut request = config
        .url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("invalid url: {e}"))?;
    let headers = request.headers_mut();
    for (key, value) in &config.headers {
        let header_name: http::HeaderName = key
            .parse()
            .map_err(|e| format!("invalid header name '{key}': {e}"))?;
        let header_value: http::HeaderValue = value
            .parse()
            .map_err(|e| format!("invalid header value: {e}"))?;
        headers.insert(header_name, header_value);
    }
    let (stream, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("ws connect failed: {e}"))?;
    Ok(stream)
}

async fn write_loop(mut sink: WsSink, mut rx: mpsc::Receiver<Message>) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = sink.send(msg).await {
            trace!("ws write error: {e}");
            break;
        }
    }
    let _ = sink.close().await;
}

async fn read_loop(
    mut source: WsSource,
    conn_id: ConnectionId,
    config: Arc<WsConfig>,
    owner_binary: ComponentBinary,
    cancel_token: CancellationToken,
    manager: Arc<WsManager>,
) {
    loop {
        let disconnected = match source.next().await {
            Some(Ok(Message::Binary(data))) => {
                dispatch_export("on-message", (conn_id, data.to_vec()), &owner_binary, &manager)
                    .await;
                false
            }
            Some(Ok(Message::Text(text))) => {
                dispatch_export(
                    "on-message",
                    (conn_id, text.as_bytes().to_vec()),
                    &owner_binary,
                    &manager,
                )
                .await;
                false
            }
            Some(Ok(Message::Close(frame))) => {
                let (code, reason) = match frame {
                    Some(f) => (f.code.into(), f.reason.to_string()),
                    None => (1000u16, String::new()),
                };
                dispatch_export("on-close", (conn_id, code, reason), &owner_binary, &manager)
                    .await;
                true
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => false,
            Some(Err(e)) => {
                dispatch_export("on-error", (conn_id, e.to_string()), &owner_binary, &manager)
                    .await;
                true
            }
            None => {
                dispatch_export(
                    "on-close",
                    (conn_id, 1006u16, "connection lost".to_owned()),
                    &owner_binary,
                    &manager,
                )
                .await;
                true
            }
        };
        if disconnected {
            if !config.auto_reconnect || cancel_token.is_cancelled() {
                break;
            }
            match reconnect(conn_id, &config, &cancel_token, &manager).await {
                Some(new_source) => source = new_source,
                None => break,
            }
        }
    }
}

async fn reconnect(
    conn_id: ConnectionId,
    config: &WsConfig,
    cancel_token: &CancellationToken,
    manager: &Arc<WsManager>,
) -> Option<WsSource> {
    let mut delay = std::time::Duration::from_secs(1);
    let max_delay = std::time::Duration::from_secs(30);
    loop {
        info!("ws connection {conn_id} reconnecting in {delay:?}");
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = cancel_token.cancelled() => {
                trace!("ws connection {conn_id} reconnect cancelled");
                return None;
            }
        }
        match open_ws_connection(config).await {
            Ok(stream) => {
                let (sink, source) = stream.split();
                let (write_tx, write_rx) = mpsc::channel::<Message>(64);
                tokio::spawn(write_loop(sink, write_rx));
                manager.replace_writer(conn_id, write_tx).await;
                info!("ws connection {conn_id} reconnected");
                return Some(source);
            }
            Err(e) => {
                warn!("ws connection {conn_id} reconnect failed: {e}");
                delay = (delay * 2).min(max_delay);
            }
        }
    }
}

const INCOMING_HANDLER_EXPORT: &str = "asterai:host-ws/incoming-handler@0.1.0";

/// Dispatches a call to a typed export on the owning component's instance.
async fn dispatch_export<Params>(
    func_name: &'static str,
    params: Params,
    owner_binary: &ComponentBinary,
    manager: &WsManager,
) where
    Params: ComponentNamedList + Lower + Send + Sync + 'static,
{
    let result = dispatch_callback(owner_binary, manager, |store, instance| {
        Box::pin(async move {
            let func: TypedFunc<Params, ()> = get_export_func(store, instance, func_name)?;
            func.call_async(&mut *store, params)
                .await
                .map_err(|e| eyre!("{e:#}"))?;
            func.post_return_async(&mut *store)
                .await
                .map_err(|e| eyre!("{e:#}"))?;
            Ok(())
        })
    })
    .await;
    if let Err(e) = result {
        error!("ws {func_name} dispatch failed: {e:#}");
    }
}

/// Lock the shared store, find the existing instance for the owning component,
/// and call a callback function.
/// This preserves component state across calls.
async fn dispatch_callback<F>(
    owner_binary: &ComponentBinary,
    manager: &WsManager,
    callback: F,
) -> eyre::Result<()>
where
    F: for<'a> FnOnce(
        &'a mut wasmtime::Store<crate::runtime::env::HostEnv>,
        &'a wasmtime::component::Instance,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = eyre::Result<()>> + Send + 'a>,
    >,
{
    let shared_store = manager
        .shared_store()
        .ok_or_else(|| eyre!("shared store not set"))?;
    let mut store = shared_store.lock().await;
    let owner_id = owner_binary.component().id();
    // Extract the instance handle before borrowing store mutably.
    let instance = {
        let runtime_data = store
            .data()
            .runtime_data
            .as_ref()
            .ok_or_else(|| eyre!("runtime data not initialized"))?;
        runtime_data
            .instances
            .iter()
            .find(|i| i.component_interface.component().id() == owner_id)
            .ok_or_else(|| eyre!("instance not found for {}", owner_id))?
            .instance
    };
    // Set the calling component for host functions.
    let component = owner_binary.component().clone();
    *store
        .data_mut()
        .runtime_data
        .as_mut()
        .unwrap()
        .last_component
        .lock()
        .unwrap() = Some(component);
    callback(&mut store, &instance).await
}

fn get_export_func<Params, Results>(
    store: &mut wasmtime::Store<crate::runtime::env::HostEnv>,
    instance: &wasmtime::component::Instance,
    func_name: &str,
) -> eyre::Result<TypedFunc<Params, Results>>
where
    Params: wasmtime::component::ComponentNamedList + wasmtime::component::Lower,
    Results: wasmtime::component::ComponentNamedList + wasmtime::component::Lift,
{
    let (_, iface_export) = instance
        .get_export(&mut *store, None, INCOMING_HANDLER_EXPORT)
        .ok_or_else(|| eyre!("export '{INCOMING_HANDLER_EXPORT}' not found"))?;
    let (_, func_export) = instance
        .get_export(&mut *store, Some(&iface_export), func_name)
        .ok_or_else(|| eyre!("function '{func_name}' not found in '{INCOMING_HANDLER_EXPORT}'"))?;
    instance
        .get_typed_func::<Params, Results>(&mut *store, &func_export)
        .map_err(|e| eyre!("failed to get typed func '{func_name}': {e:#}"))
}
