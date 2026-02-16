use crate::runtime::entry::{execute_dynamic_call, resolve_call};
use crate::runtime::env::HostEnvRuntimeData;
use log::{error, info};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use wasmtime::component::{ComponentType, Lower};

pub type ScheduleId = u64;

struct CronSchedule {
    info: ScheduleInfo,
    cancel_token: CancellationToken,
}

#[derive(Clone)]
pub struct ScheduleInfo {
    pub id: ScheduleId,
    pub cron: String,
    pub component_name: String,
    pub function_name: String,
    pub args_json: String,
    /// The component that created this schedule.
    pub owner: String,
}

/// WIT-compatible schedule-info record for lowering into the component.
#[derive(ComponentType, Lower)]
#[component(record)]
pub struct WitScheduleInfo {
    pub id: u64,
    pub cron: String,
    #[component(name = "component-name")]
    pub component_name: String,
    #[component(name = "function-name")]
    pub function_name: String,
    #[component(name = "args-json")]
    pub args_json: String,
}

impl From<ScheduleInfo> for WitScheduleInfo {
    fn from(info: ScheduleInfo) -> Self {
        Self {
            id: info.id,
            cron: info.cron,
            component_name: info.component_name,
            function_name: info.function_name,
            args_json: info.args_json,
        }
    }
}

pub struct CronManager {
    schedules: RwLock<HashMap<ScheduleId, CronSchedule>>,
    next_id: AtomicU64,
    runtime_data: OnceLock<HostEnvRuntimeData>,
}

impl Default for CronManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CronManager {
    pub fn new() -> Self {
        Self {
            schedules: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            runtime_data: OnceLock::new(),
        }
    }

    pub fn set_runtime_data(&self, runtime_data: HostEnvRuntimeData) {
        self.runtime_data.set(runtime_data).ok();
    }

    pub async fn schedule(
        &self,
        cron_expr: String,
        component_name: String,
        function_name: String,
        args_json: String,
        owner: String,
    ) -> Result<ScheduleId, String> {
        let rd = self
            .runtime_data
            .get()
            .ok_or("cron runtime data not initialized")?;
        let normalized = normalize_cron_expr(&cron_expr)?;
        let schedule = cron::Schedule::from_str(&normalized)
            .map_err(|e| format!("invalid cron expression: {e}"))?;
        // Validate that component and function exist.
        resolve_call(
            &component_name,
            &function_name,
            &args_json,
            rd.compiled_components.iter().map(|(b, _)| b),
        )
        .map_err(|e| e.message)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel_token = CancellationToken::new();
        let info = ScheduleInfo {
            id,
            cron: cron_expr,
            component_name,
            function_name,
            args_json,
            owner,
        };
        let entry = CronSchedule {
            info: info.clone(),
            cancel_token: cancel_token.clone(),
        };
        self.schedules.write().await.insert(id, entry);
        let rd = rd.clone();
        tokio::spawn(tick_loop(schedule, info, cancel_token, rd));
        Ok(id)
    }

    pub async fn cancel(&self, id: ScheduleId, owner: &str) -> Result<(), String> {
        let mut schedules = self.schedules.write().await;
        let entry = schedules
            .get(&id)
            .ok_or_else(|| format!("schedule {id} not found"))?;
        if entry.info.owner != owner {
            return Err(format!("schedule {id} not found"));
        }
        let entry = schedules.remove(&id).unwrap();
        entry.cancel_token.cancel();
        info!("cron schedule {id} cancelled");
        Ok(())
    }

    pub async fn list(&self, owner: &str) -> Vec<ScheduleInfo> {
        self.schedules
            .read()
            .await
            .values()
            .filter(|s| s.info.owner == owner)
            .map(|s| s.info.clone())
            .collect()
    }

    pub async fn cancel_all(&self) {
        let mut schedules = self.schedules.write().await;
        for (id, entry) in schedules.drain() {
            entry.cancel_token.cancel();
            info!("cron schedule {id} cancelled");
        }
    }
}

async fn tick_loop(
    schedule: cron::Schedule,
    info: ScheduleInfo,
    cancel_token: CancellationToken,
    rd: HostEnvRuntimeData,
) {
    info!(
        "cron schedule {} started: '{}' -> {}/{}",
        info.id, info.cron, info.component_name, info.function_name
    );
    loop {
        let next = schedule.upcoming(chrono::Utc).next();
        let Some(next_time) = next else {
            info!("cron schedule {}: no more upcoming times", info.id);
            break;
        };
        let now = chrono::Utc::now();
        let duration = (next_time - now).to_std().unwrap_or_default();
        tokio::select! {
            _ = tokio::time::sleep(duration) => {}
            _ = cancel_token.cancelled() => {
                break;
            }
        }
        if cancel_token.is_cancelled() {
            break;
        }
        execute_cron_call(&info, &rd).await;
    }
}

async fn execute_cron_call(info: &ScheduleInfo, rd: &HostEnvRuntimeData) {
    let compiled_components = rd.compiled_components.clone();
    let env_vars = rd.env_vars.clone();
    let preopened_dirs = rd.preopened_dirs.clone();
    let runtime_data = rd.clone();
    let component_name = info.component_name.clone();
    let function_name = info.function_name.clone();
    let args_json = info.args_json.clone();
    let result = tokio::task::spawn_blocking(move || {
        let (comp_id, function, inputs) = resolve_call(
            &component_name,
            &function_name,
            &args_json,
            compiled_components.iter().map(|(b, _)| b),
        )?;
        execute_dynamic_call(
            compiled_components,
            comp_id,
            function,
            inputs,
            env_vars,
            preopened_dirs,
            runtime_data,
        )
    })
    .await;
    match result {
        Ok(Ok(output)) => {
            info!("cron schedule {} executed: {}", info.id, output);
        }
        Ok(Err(e)) => {
            error!("cron schedule {} call failed: {}", info.id, e.message);
        }
        Err(e) => {
            error!("cron schedule {} task panicked: {e}", info.id);
        }
    }
}

/// Normalizes a cron expression to the 7-field format the `cron` crate expects
/// (sec min hour dom month dow year).
/// - 5 fields (standard cron): prepends `0` for seconds, appends `*` for year.
/// - 6 fields (with seconds): appends `*` for year.
/// - 7 fields: used as-is.
fn normalize_cron_expr(expr: &str) -> Result<String, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {expr} *")),
        6 => Ok(format!("{expr} *")),
        7 => Ok(expr.to_owned()),
        n => Err(format!("expected 5, 6, or 7 cron fields, got {n}")),
    }
}
