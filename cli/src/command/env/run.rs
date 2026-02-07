use crate::auth::Auth;
use crate::command::env::call_api::{AppState, RUNTIME_SECRET_ENV, handle_call};
use crate::command::resource_or_id::ResourceOrIdArg;
use crate::local_store::LocalStore;
use crate::registry::{GetEnvironmentResponse, RegistryClient};
use crate::runtime::build_runtime;
use asterai_runtime::component::Component;
use asterai_runtime::environment::{Environment, EnvironmentMetadata};
use asterai_runtime::resource::metadata::ResourceKind;
use asterai_runtime::runtime::http::{self, HttpRouteTable};
use axum::extract::State;
use axum::response::IntoResponse;
use eyre::{Context, OptionExt, bail};
use http_body_util::BodyExt;
use hyper::StatusCode as HyperStatusCode;
use reqwest::StatusCode;
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
pub(super) struct RunArgs {
    /// Environment reference (name, namespace:name, or namespace:name@version).
    env_ref: ResourceOrIdArg,
    /// If true, don't pull from registry - use cached version only.
    no_pull: bool,
    port: u16,
    host: String,
}

impl RunArgs {
    pub fn parse(args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut env_ref: Option<ResourceOrIdArg> = None;
        let mut no_pull = false;
        let mut port: u16 = 8080;
        let mut host = "127.0.0.1".to_string();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--no-pull" => {
                    no_pull = true;
                }
                "--port" | "-p" => {
                    let val = args
                        .next()
                        .ok_or_else(|| eyre::eyre!("--port requires a value"))?;
                    port = val
                        .parse()
                        .map_err(|_| eyre::eyre!("invalid port: {}", val))?;
                }
                "--host" => {
                    host = args
                        .next()
                        .ok_or_else(|| eyre::eyre!("--host requires a value"))?;
                }
                "--help" | "-h" | "help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if env_ref.is_some() {
                        bail!("unexpected argument: {}", other);
                    }
                    env_ref = Some(ResourceOrIdArg::from_str(other).unwrap());
                }
            }
        }
        let env_ref = env_ref.ok_or_eyre(
            "missing environment reference\n\n\
             Usage: asterai env run <name[@version]>\n\
             Example: asterai env run my-env",
        )?;
        Ok(Self {
            env_ref,
            no_pull,
            port,
            host,
        })
    }

    pub async fn execute(&self, api_endpoint: &str, registry_endpoint: &str) -> eyre::Result<()> {
        let namespace = self.env_ref.resolved_namespace();
        let name = self.env_ref.name();
        let version = self.env_ref.version().map(|v| v.to_string());
        // Try to find environment locally first.
        let local_env = self.find_local_environment(&namespace, name, version.as_deref());
        let environment = match local_env {
            Some(env) => {
                println!(
                    "running environment {}:{}@{} (cached)",
                    env.namespace(),
                    env.name(),
                    env.version()
                );
                env
            }
            None => {
                if self.no_pull {
                    bail!(
                        "environment '{}:{}{}' not found locally \
                         (use without --no-pull to fetch from registry)",
                        namespace,
                        name,
                        version
                            .as_ref()
                            .map(|v| format!("@{}", v))
                            .unwrap_or_default()
                    );
                }
                // Pull from registry.
                self.pull_environment(
                    &namespace,
                    name,
                    version.as_deref(),
                    api_endpoint,
                    registry_endpoint,
                )
                .await?
            }
        };
        // Run the environment.
        let runtime = build_runtime(environment).await?;
        let route_table = runtime.http_route_table();
        let runtime = Arc::new(Mutex::new(runtime));
        // Always start the HTTP server (call API + component routes).
        let addr: SocketAddr = format!("{}:{}", self.host, self.port).parse()?;
        let runtime_secret = std::env::var(RUNTIME_SECRET_ENV).ok();
        if runtime_secret.is_some() {
            println!("call API authentication enabled ({RUNTIME_SECRET_ENV} is set)");
        }
        let state = AppState {
            route_table: route_table.clone(),
            runtime: runtime.clone(),
            runtime_secret,
        };
        let app = axum::Router::new()
            .route("/health", axum::routing::get(|| async { "ok" }))
            .route(
                "/v1/environment/{env_ns}/{env_name}/call",
                axum::routing::post(handle_call),
            )
            .fallback(handle_request)
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        print_routes(&route_table, &addr);
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("http server error: {e}");
            }
        });
        // Run CLI components (wasi:cli/run).
        {
            let mut rt = runtime.lock().await;
            rt.run().await?;
        }
        // Keep alive for the HTTP server.
        tokio::signal::ctrl_c().await?;
        Ok(())
    }

    fn find_local_environment(
        &self,
        namespace: &str,
        name: &str,
        version: Option<&str>,
    ) -> Option<Environment> {
        let local_envs = LocalStore::list_environments();
        if let Some(ver) = version {
            // Look for specific version.
            local_envs.into_iter().find(|env| {
                env.namespace() == namespace && env.name() == name && env.version() == ver
            })
        } else {
            // Find latest local version for this namespace:name.
            local_envs
                .into_iter()
                .filter(|env| env.namespace() == namespace && env.name() == name)
                .max_by(|a, b| {
                    // Compare versions using semver if possible.
                    let ver_a = semver::Version::parse(a.version()).ok();
                    let ver_b = semver::Version::parse(b.version()).ok();
                    match (ver_a, ver_b) {
                        (Some(va), Some(vb)) => va.cmp(&vb),
                        _ => a.version().cmp(b.version()),
                    }
                })
        }
    }

    async fn pull_environment(
        &self,
        namespace: &str,
        name: &str,
        version: Option<&str>,
        api_endpoint: &str,
        registry_endpoint: &str,
    ) -> eyre::Result<Environment> {
        let api_key = Auth::read_stored_api_key()
            .ok_or_eyre("API key not found. Run 'asterai auth login' to authenticate.")?;
        println!(
            "pulling environment {}:{}{}...",
            namespace,
            name,
            version.map(|v| format!("@{}", v)).unwrap_or_default()
        );
        // Fetch environment from API.
        let client = reqwest::Client::new();
        let url = match version {
            Some(ver) => format!(
                "{}/v1/environment/{}/{}/{}",
                api_endpoint, namespace, name, ver
            ),
            None => format!("{}/v1/environment/{}/{}", api_endpoint, namespace, name),
        };
        let response = client
            .get(&url)
            .header("Authorization", api_key.trim())
            .send()
            .await
            .wrap_err("failed to fetch environment")?;
        if response.status() == StatusCode::NOT_FOUND {
            bail!("environment '{}:{}' not found in registry", namespace, name);
        }
        if response.status() == StatusCode::FORBIDDEN {
            bail!(
                "forbidden: you don't have access to environment '{}:{}'",
                namespace,
                name
            );
        }
        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to fetch environment: {}", error_text);
        }
        let env_data: GetEnvironmentResponse = response
            .json()
            .await
            .wrap_err("failed to parse environment response")?;
        println!("  version: {}", env_data.version);
        println!("  components: {}", env_data.components.len());
        // Parse component refs into components map.
        let mut components_map: HashMap<String, String> = HashMap::new();
        let mut component_list: Vec<Component> = Vec::new();
        for comp_ref in &env_data.components {
            let component = Component::from_str(comp_ref)
                .wrap_err_with(|| format!("failed to parse component: {}", comp_ref))?;
            let key = format!("{}:{}", component.namespace(), component.name());
            components_map.insert(key, component.version().to_string());
            component_list.push(component);
        }
        // Create local environment.
        let environment = Environment {
            metadata: EnvironmentMetadata {
                namespace: env_data.namespace.clone(),
                name: env_data.name.clone(),
                version: env_data.version.clone(),
            },
            components: components_map,
            vars: env_data.vars,
        };
        LocalStore::write_environment(&environment)?;
        // Write additional metadata (pulled_from).
        let env_dir = LocalStore::environment_dir(&environment);
        let metadata_path = env_dir.join("metadata.json");
        let metadata = serde_json::json!({
            "kind": ResourceKind::Environment.to_string(),
            "pulled_from": format!("{}:{}@{}", env_data.namespace, env_data.name, env_data.version),
        });
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;
        println!("  saved to {}", env_dir.display());
        // Pull component WASMs using shared registry client.
        println!("\npulling components...");
        let registry = RegistryClient::new(&client, api_endpoint, registry_endpoint);
        for component in &component_list {
            registry
                .pull_component(Some(&api_key), component, false)
                .await?;
        }
        Ok(environment)
    }
}

async fn handle_request(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let route_table = &state.route_table;
    let path = req.uri().path().to_string();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 4 {
        return (
            HyperStatusCode::NOT_FOUND,
            "not found: expected \
             /:env-namespace/:env-name/:comp-namespace/:comp-name/...",
        )
            .into_response();
    }
    let env_ns = segments[0];
    let env_name = segments[1];
    let comp_ns = segments[2];
    let comp_name = segments[3];
    if env_ns != route_table.env_namespace() || env_name != route_table.env_name() {
        return (
            HyperStatusCode::NOT_FOUND,
            format!("no environment at /{env_ns}/{env_name}"),
        )
            .into_response();
    }
    let route = match route_table.lookup(comp_ns, comp_name) {
        Some(r) => r.clone(),
        None => {
            return (
                HyperStatusCode::NOT_FOUND,
                format!("no component at /{env_ns}/{env_name}/{comp_ns}/{comp_name}"),
            )
                .into_response();
        }
    };
    let (mut parts, body) = req.into_parts();
    parts.uri = http::strip_path_prefix(&parts.uri, env_ns, env_name, comp_ns, comp_name);
    // Collect body bytes and re-wrap for wasmtime compatibility.
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            eprintln!("error reading request body: {e:#}");
            return (HyperStatusCode::BAD_REQUEST, "failed to read request body").into_response();
        }
    };
    let full_body = http_body_util::Full::new(body_bytes).map_err(|never| match never {});
    let hyper_req = hyper::Request::from_parts(parts, full_body);
    match http::handle_http_request(&route, route_table.env_vars(), hyper_req).await {
        Ok(resp) => resp.into_response(),
        Err(e) => {
            eprintln!("error handling request: {e:#}");
            (HyperStatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response()
        }
    }
}

fn print_routes(route_table: &HttpRouteTable, addr: &SocketAddr) {
    let env_ns = route_table.env_namespace();
    let env_name = route_table.env_name();
    println!("listening on http://{addr}");
    for (comp_path, route) in route_table.routes() {
        println!("  /{env_ns}/{env_name}/{comp_path} -> {}", route.component);
    }
}

fn print_help() {
    println!(
        r#"Run an environment locally.

Usage: asterai env run <name[@version]> [options]
       asterai env run <namespace:name[@version]> [options]

Arguments:
  <[namespace:]name[@version]>  Environment reference
                                Namespace defaults to your account namespace
                                Version defaults to latest available

Options:
  --no-pull             Don't pull from registry, use cached version only
  -p, --port <port>     HTTP server port (default: 8080)
  --host <host>         HTTP server host (default: 127.0.0.1)
  -h, --help            Show this help message

Examples:
  asterai env run my-env                    # Run latest, default namespace
  asterai env run myteam:my-env             # Pull (if needed) and run latest
  asterai env run myteam:my-env@1.2.0       # Pull (if needed) and run specific version
  asterai env run my-env --no-pull          # Run cached version only
  asterai env run my-env -p 3000            # Run with HTTP server on port 3000
"#
    );
}
