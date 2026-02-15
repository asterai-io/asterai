use crate::component::Component;
use crate::runtime::env::{HostEnv, HostEnvRuntimeData, create_fresh_store};
use crate::runtime::wasm_instance::ENGINE;
use bytes::Bytes;
use eyre::eyre;
use http_body::Body;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use wasmtime_wasi_http::WasiHttpView;
use wasmtime_wasi_http::bindings::ProxyPre;
use wasmtime_wasi_http::bindings::http::types::Scheme;
use wasmtime_wasi_http::body::HyperOutgoingBody;

pub struct HttpRoute {
    pub component: Component,
    pub proxy_pre: ProxyPre<HostEnv>,
}

pub struct HttpRouteTable {
    routes: HashMap<String, Arc<HttpRoute>>,
    env_vars: HashMap<String, String>,
    preopened_dirs: Vec<PathBuf>,
    env_namespace: String,
    env_name: String,
    runtime_data: Option<HostEnvRuntimeData>,
}

impl HttpRouteTable {
    pub fn new(
        routes: HashMap<String, Arc<HttpRoute>>,
        env_vars: HashMap<String, String>,
        preopened_dirs: Vec<PathBuf>,
        env_namespace: String,
        env_name: String,
        runtime_data: Option<HostEnvRuntimeData>,
    ) -> Self {
        Self {
            routes,
            env_vars,
            preopened_dirs,
            env_namespace,
            env_name,
            runtime_data,
        }
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

    pub fn preopened_dirs(&self) -> &[PathBuf] {
        &self.preopened_dirs
    }

    pub fn env_namespace(&self) -> &str {
        &self.env_namespace
    }

    pub fn env_name(&self) -> &str {
        &self.env_name
    }

    pub fn runtime_data(&self) -> Option<&HostEnvRuntimeData> {
        self.runtime_data.as_ref()
    }
}

pub async fn handle_http_request<B>(
    route: &HttpRoute,
    env_vars: &HashMap<String, String>,
    preopened_dirs: &[PathBuf],
    runtime_data: Option<&HostEnvRuntimeData>,
    req: hyper::Request<B>,
) -> eyre::Result<hyper::Response<HyperOutgoingBody>>
where
    B: Body<Data = Bytes, Error = hyper::Error> + Send + 'static,
{
    let engine = &*ENGINE;
    let mut store = create_fresh_store(engine, env_vars, preopened_dirs);
    // Populate runtime data in the new store.
    if let Some(rd) = runtime_data {
        store.data_mut().runtime_data = Some(rd.clone());
    }
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

pub fn strip_path_prefix(
    uri: &hyper::Uri,
    env_namespace: &str,
    env_name: &str,
    comp_namespace: &str,
    comp_name: &str,
) -> hyper::Uri {
    let path = uri.path();
    let prefix = format!("/{env_namespace}/{env_name}/{comp_namespace}/{comp_name}");
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
