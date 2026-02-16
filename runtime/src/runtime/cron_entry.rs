//! Host entry points for the asterai cron scheduling interface.
use crate::runtime::cron::{CronManager, WitScheduleInfo};
use crate::runtime::env::HostEnv;
use std::future::Future;
use std::sync::Arc;
use wasmtime::StoreContextMut;
use wasmtime::component::Linker;

type HostFuture<'a, T> = Box<dyn Future<Output = Result<T, wasmtime::Error>> + Send + 'a>;

pub fn add_asterai_cron_to_linker(linker: &mut Linker<HostEnv>) -> eyre::Result<()> {
    let mut instance = linker
        .instance("asterai:host-cron/scheduler@0.1.0")
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("create-schedule", cron_schedule)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("cancel-schedule", cron_cancel)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap_async("list-schedules", cron_list)
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    Ok(())
}

fn cron_schedule<'a>(
    store: StoreContextMut<'a, HostEnv>,
    (cron_expr, component_name, function_name, args_json): (String, String, String, String),
) -> HostFuture<'a, (Result<u64, String>,)> {
    Box::new(async move {
        let mgr = match get_cron_manager(&store) {
            Ok(m) => m,
            Err(e) => return Ok((Err(e),)),
        };
        let owner = match get_caller_name(&store) {
            Ok(n) => n,
            Err(e) => return Ok((Err(e),)),
        };
        let result = mgr
            .schedule(cron_expr, component_name, function_name, args_json, owner)
            .await;
        Ok((result,))
    })
}

fn cron_cancel<'a>(
    store: StoreContextMut<'a, HostEnv>,
    (id,): (u64,),
) -> HostFuture<'a, (Result<(), String>,)> {
    Box::new(async move {
        let mgr = match get_cron_manager(&store) {
            Ok(m) => m,
            Err(e) => return Ok((Err(e),)),
        };
        let owner = match get_caller_name(&store) {
            Ok(n) => n,
            Err(e) => return Ok((Err(e),)),
        };
        let result = mgr.cancel(id, &owner).await;
        Ok((result,))
    })
}

fn cron_list<'a>(
    store: StoreContextMut<'a, HostEnv>,
    _params: (),
) -> HostFuture<'a, (Vec<WitScheduleInfo>,)> {
    Box::new(async move {
        let mgr = match get_cron_manager(&store) {
            Ok(m) => m,
            Err(_) => return Ok((Vec::new(),)),
        };
        let owner = match get_caller_name(&store) {
            Ok(n) => n,
            Err(_) => return Ok((Vec::new(),)),
        };
        let infos: Vec<WitScheduleInfo> = mgr
            .list(&owner)
            .await
            .into_iter()
            .map(WitScheduleInfo::from)
            .collect();
        Ok((infos,))
    })
}

fn get_cron_manager(store: &StoreContextMut<HostEnv>) -> Result<Arc<CronManager>, String> {
    store
        .data()
        .runtime_data
        .as_ref()
        .and_then(|rd| rd.cron_manager.clone())
        .ok_or_else(|| "cron manager not available".to_owned())
}

fn get_caller_name(store: &StoreContextMut<HostEnv>) -> Result<String, String> {
    store
        .data()
        .runtime_data
        .as_ref()
        .and_then(|rd| rd.last_component.lock().ok())
        .and_then(|guard| guard.as_ref().map(|c| c.id().to_string()))
        .ok_or_else(|| "unknown caller component".to_owned())
}

pub fn add_asterai_cron_to_sync_linker(linker: &mut Linker<HostEnv>) -> eyre::Result<()> {
    let mut instance = linker
        .instance("asterai:host-cron/scheduler@0.1.0")
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap(
            "create-schedule",
            |store: StoreContextMut<HostEnv>,
             (cron_expr, component_name, function_name, args_json): (
                String,
                String,
                String,
                String,
            )| {
                let mgr = match get_cron_manager(&store) {
                    Ok(m) => m,
                    Err(e) => return Ok((Err(e),)),
                };
                let owner = match get_caller_name(&store) {
                    Ok(n) => n,
                    Err(e) => return Ok((Err(e),)),
                };
                let handle = tokio::runtime::Handle::current();
                let result = handle.block_on(mgr.schedule(
                    cron_expr,
                    component_name,
                    function_name,
                    args_json,
                    owner,
                ));
                Ok((result,))
            },
        )
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap(
            "cancel-schedule",
            |store: StoreContextMut<HostEnv>, (id,): (u64,)| {
                let mgr = match get_cron_manager(&store) {
                    Ok(m) => m,
                    Err(e) => return Ok((Err(e),)),
                };
                let owner = match get_caller_name(&store) {
                    Ok(n) => n,
                    Err(e) => return Ok((Err(e),)),
                };
                let handle = tokio::runtime::Handle::current();
                let result = handle.block_on(mgr.cancel(id, &owner));
                Ok((result,))
            },
        )
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    instance
        .func_wrap(
            "list-schedules",
            |store: StoreContextMut<HostEnv>, _params: ()| {
                let mgr = match get_cron_manager(&store) {
                    Ok(m) => m,
                    Err(_) => return Ok((Vec::<WitScheduleInfo>::new(),)),
                };
                let owner = match get_caller_name(&store) {
                    Ok(n) => n,
                    Err(_) => return Ok((Vec::<WitScheduleInfo>::new(),)),
                };
                let handle = tokio::runtime::Handle::current();
                let infos: Vec<WitScheduleInfo> = handle
                    .block_on(mgr.list(&owner))
                    .into_iter()
                    .map(WitScheduleInfo::from)
                    .collect();
                Ok((infos,))
            },
        )
        .map_err(|e| eyre::eyre!("{e:#?}"))?;
    Ok(())
}
