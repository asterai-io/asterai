use crate::component::Component;
use crate::runtime::env::HostEnv;
use crate::runtime::std_out_err::{ComponentStderr, ComponentStdout};
use crate::runtime::wasm_instance::ENGINE;
use bytes::Bytes;
use eyre::eyre;
use http_body::Body;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;
use wasmtime::Store;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi_http::bindings::ProxyPre;
use wasmtime_wasi_http::bindings::http::types::Scheme;
use wasmtime_wasi_http::body::HyperOutgoingBody;
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

pub struct HttpRoute {
    pub component: Component,
    pub proxy_pre: ProxyPre<HostEnv>,
}

pub struct HttpRouteTable {
    routes: HashMap<String, Arc<HttpRoute>>,
    env_vars: HashMap<String, String>,
}

impl HttpRouteTable {
    pub fn new(routes: HashMap<String, Arc<HttpRoute>>, env_vars: HashMap<String, String>) -> Self {
        Self { routes, env_vars }
    }

    pub fn lookup(&self, namespace: &str, name: &str) -> Option<&Arc<HttpRoute>> {
        let key = format!("{namespace}/{name}");
        self.routes.get(&key)
    }

    pub fn routes(&self) -> &HashMap<String, Arc<HttpRoute>> {
        &self.routes
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub fn env_vars(&self) -> &HashMap<String, String> {
        &self.env_vars
    }
}

pub async fn handle_http_request<B>(
    route: &HttpRoute,
    env_vars: &HashMap<String, String>,
    req: hyper::Request<B>,
) -> eyre::Result<hyper::Response<HyperOutgoingBody>>
where
    B: Body<Data = Bytes, Error = hyper::Error> + Send + 'static,
{
    let engine = &*ENGINE;
    let app_id = Uuid::new_v4();
    let mut wasi_ctx_builder = WasiCtxBuilder::new();
    wasi_ctx_builder
        .stdout(ComponentStdout { app_id })
        .stderr(ComponentStderr { app_id })
        .inherit_network();
    for (key, value) in env_vars {
        wasi_ctx_builder.env(key, value);
    }
    let wasi_ctx = wasi_ctx_builder.build();
    let (component_output_tx, mut component_output_rx) = mpsc::channel(32);
    tokio::spawn(async move { while component_output_rx.recv().await.is_some() {} });
    let host_env = HostEnv {
        runtime_data: None,
        wasi_ctx,
        http_ctx: WasiHttpCtx::new(),
        table: ResourceTable::new(),
        component_output_tx,
    };
    let mut store = Store::new(engine, host_env);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let req = store
        .data_mut()
        .new_incoming_request(Scheme::Http, req)
        .map_err(|e| eyre!(e))?;
    let out = store
        .data_mut()
        .new_response_outparam(sender)
        .map_err(|e| eyre!(e))?;
    let pre = route.proxy_pre.clone();
    let task = tokio::task::spawn(async move {
        let proxy = pre.instantiate_async(&mut store).await?;
        proxy
            .wasi_http_incoming_handler()
            .call_handle(store, req, out)
            .await?;
        Ok::<(), anyhow::Error>(())
    });
    match receiver.await {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => Err(eyre!("{e:?}")),
        Err(_) => {
            let e = match task.await {
                Ok(Ok(())) => {
                    return Err(eyre!("guest never invoked response-outparam::set"));
                }
                Ok(Err(e)) => e,
                Err(e) => e.into(),
            };
            Err(eyre!(e).wrap_err("guest never invoked response-outparam::set"))
        }
    }
}

pub fn strip_path_prefix(uri: &hyper::Uri, namespace: &str, name: &str) -> hyper::Uri {
    let path = uri.path();
    let prefix = format!("/{namespace}/{name}");
    let stripped = path.strip_prefix(&prefix).unwrap_or(path);
    let stripped = match stripped.is_empty() {
        true => "/",
        false => stripped,
    };
    let path_and_query = match uri.query() {
        Some(q) => format!("{stripped}?{q}"),
        None => stripped.to_string(),
    };
    hyper::Uri::builder()
        .path_and_query(path_and_query)
        .build()
        .unwrap_or_else(|_| uri.clone())
}
