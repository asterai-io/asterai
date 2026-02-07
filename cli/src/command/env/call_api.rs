use asterai_runtime::component::ComponentId;
use asterai_runtime::component::function_name::ComponentFunctionName;
use asterai_runtime::runtime::ComponentRuntime;
use asterai_runtime::runtime::http::HttpRouteTable;
use asterai_runtime::runtime::parsing::{ValExt, json_value_to_val_typedef};
use axum::extract::State;
use axum::response::IntoResponse;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub route_table: Arc<HttpRouteTable>,
    pub runtime: Arc<Mutex<ComponentRuntime>>,
}

#[derive(Deserialize)]
pub struct CallRequest {
    component: String,
    function: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct CallResponse {
    output: Option<serde_json::Value>,
}

pub async fn handle_call(
    State(state): State<AppState>,
    axum::extract::Path((env_ns, env_name)): axum::extract::Path<(String, String)>,
    axum::Json(body): axum::Json<CallRequest>,
) -> impl IntoResponse {
    match handle_call_inner(&state, &env_ns, &env_name, body).await {
        Ok(response) => (StatusCode::OK, axum::Json(response)).into_response(),
        Err(e) => {
            let msg = format!("{e:#}");
            let status = match msg.contains("not found") {
                true => StatusCode::NOT_FOUND,
                false => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, msg).into_response()
        }
    }
}

async fn handle_call_inner(
    state: &AppState,
    env_ns: &str,
    env_name: &str,
    body: CallRequest,
) -> eyre::Result<CallResponse> {
    if env_ns != state.route_table.env_namespace() || env_name != state.route_table.env_name() {
        eyre::bail!("environment {env_ns}:{env_name} not found");
    }
    let comp_id = ComponentId::from_str(&body.component)
        .map_err(|e| eyre::eyre!("invalid component: {e}"))?;
    let function_name = ComponentFunctionName::from_str(&body.function).unwrap();
    let mut runtime = state.runtime.lock().await;
    let function = runtime
        .find_function(&comp_id, &function_name, None)
        .ok_or_else(|| {
            eyre::eyre!(
                "function '{}' not found on component '{}'",
                body.function,
                body.component
            )
        })?;
    if body.args.len() != function.inputs.len() {
        eyre::bail!(
            "expected {} argument(s), got {}",
            function.inputs.len(),
            body.args.len()
        );
    }
    let inputs = body
        .args
        .iter()
        .zip(function.inputs.iter())
        .map(|(arg, (_name, type_def))| json_value_to_val_typedef(arg, type_def))
        .collect::<eyre::Result<Vec<_>>>()?;
    let output_opt = runtime.call_function(function, &inputs).await?;
    let output = output_opt
        .and_then(|o| o.function_output_opt)
        .and_then(|o| o.value.val.try_into_json_value());
    Ok(CallResponse { output })
}
