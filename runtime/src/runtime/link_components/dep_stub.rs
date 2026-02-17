//! Pre-registers forwarding stubs in the linker for component-to-component
//! exports, allowing instantiation in any order (including cycles).
use crate::component::Component;
use crate::component::binary::ComponentBinary;
use crate::component::function_interface::ComponentFunctionInterface;
use crate::runtime::entry::instantiate_all_sync;
use crate::runtime::env::{HostEnv, create_fresh_store, create_sync_linker};
use crate::runtime::wasm_instance::SYNC_ENGINE;
use eyre::eyre;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use wasmtime::AsContextMut;
use wasmtime::component::{Func, Instance, Linker, LinkerInstance, Val};

/// Resolved function slot populated after component instantiation.
struct ResolvedFunc {
    func: Func,
    component: Component,
    function_info: ComponentFunctionInterface,
}

type FuncSlot = Arc<OnceLock<ResolvedFunc>>;

/// Key for a function slot: (instance_export_name, function_name).
type SlotKey = (String, String);

/// Opaque handle returned by [`register_component_stubs`],
/// passed to [`resolve_component_stubs`] after each instantiation.
pub struct ComponentStubs {
    slots: HashMap<SlotKey, FuncSlot>,
}

/// Pre-registers forwarding stubs in the linker for every function
/// exported by any component. Each stub delegates to the sync engine
/// on a blocking thread to avoid the nested `run_concurrent` assertion
/// that occurs when `call_async` is used inside an active guest thread.
pub fn register_component_stubs(
    components: &[ComponentBinary],
    linker: &mut Linker<HostEnv>,
) -> eyre::Result<ComponentStubs> {
    for_each_stub(components, linker, |inst_builder, f, slot| {
        inst_builder
            .func_new_async(&f.name.name, move |store, _, params, results| {
                let slot = slot.clone();
                Box::new(async move {
                    let resolved = slot
                        .get()
                        .ok_or_else(|| wasmtime::Error::msg("unresolved component function"))?;
                    let rd = store
                        .data()
                        .runtime_data
                        .as_ref()
                        .ok_or_else(|| wasmtime::Error::msg("runtime not initialized"))?;
                    let compiled_components = rd.compiled_components.clone();
                    let env_vars = rd.env_vars.clone();
                    let preopened_dirs = rd.preopened_dirs.clone();
                    let runtime_data = rd.clone();
                    let comp_id = resolved.component.id().clone();
                    let function = resolved.function_info.clone();
                    let inputs: Vec<Val> = params.to_vec();
                    let sync_results = tokio::task::spawn_blocking(move || {
                        execute_stub_call(
                            &compiled_components,
                            &comp_id,
                            &function,
                            &inputs,
                            &env_vars,
                            &preopened_dirs,
                            runtime_data,
                        )
                    })
                    .await
                    .map_err(|e| wasmtime::Error::msg(format!("{e}")))?
                    .map_err(|e| wasmtime::Error::msg(format!("{e:#}")))?;
                    for (i, val) in sync_results.into_iter().enumerate() {
                        if i < results.len() {
                            results[i] = val;
                        }
                    }
                    Ok(())
                })
            })
            .map_err(|e| eyre!("{e:#?}"))
    })
}

/// Sync variant of [`register_component_stubs`] for the sync engine.
/// Stubs use `Func::call` (sync) instead of `call_async`, avoiding
/// the nested `run_concurrent` assertion.
pub fn register_component_stubs_sync(
    components: &[ComponentBinary],
    linker: &mut Linker<HostEnv>,
) -> eyre::Result<ComponentStubs> {
    for_each_stub(components, linker, |inst_builder, f, slot| {
        inst_builder
            .func_new(&f.name.name, move |mut store, _, params, results| {
                let resolved = slot
                    .get()
                    .ok_or_else(|| wasmtime::Error::msg("unresolved component function"))?;
                resolved.func.call(&mut store, params, results)?;
                resolved.func.post_return(&mut store)?;
                Ok(())
            })
            .map_err(|e| eyre!("{e:#?}"))
    })
}

/// Iterates all exported functions grouped by instance, creates an
/// `OnceLock` slot for each, and delegates the actual linker
/// registration to `register`.
fn for_each_stub<F>(
    components: &[ComponentBinary],
    linker: &mut Linker<HostEnv>,
    mut register: F,
) -> eyre::Result<ComponentStubs>
where
    F: FnMut(
        &mut LinkerInstance<'_, HostEnv>,
        &ComponentFunctionInterface,
        FuncSlot,
    ) -> eyre::Result<()>,
{
    let mut slots: HashMap<SlotKey, FuncSlot> = HashMap::new();
    for (inst_name, funcs) in group_exports_by_instance(components) {
        let mut inst_builder = linker.instance(&inst_name).map_err(|e| eyre!("{e:#?}"))?;
        for f in funcs {
            let key = (inst_name.clone(), f.name.name.clone());
            let slot: FuncSlot = Arc::new(OnceLock::new());
            register(&mut inst_builder, &f, slot.clone())?;
            slots.insert(key, slot);
        }
    }
    Ok(ComponentStubs { slots })
}

/// Groups all exported functions by their instance export name
/// across all components.
fn group_exports_by_instance(
    components: &[ComponentBinary],
) -> HashMap<String, Vec<ComponentFunctionInterface>> {
    let mut by_instance: HashMap<String, Vec<ComponentFunctionInterface>> = HashMap::new();
    for comp in components {
        for f in comp.get_functions() {
            if let Some(inst) = f.get_instance_export_name() {
                by_instance.entry(inst).or_default().push(f);
            }
        }
    }
    by_instance
}

/// Runs a cross-component call on a blocking thread with the sync engine.
/// This avoids the nested `run_concurrent` assertion by using `Func::call`
/// (sync) instead of `Func::call_async`.
fn execute_stub_call(
    compiled_components: &[(ComponentBinary, wasmtime::component::Component)],
    comp_id: &crate::component::ComponentId,
    function: &ComponentFunctionInterface,
    inputs: &[Val],
    env_vars: &HashMap<String, String>,
    preopened_dirs: &[std::path::PathBuf],
    runtime_data: crate::runtime::env::HostEnvRuntimeData,
) -> eyre::Result<Vec<Val>> {
    let engine = &*SYNC_ENGINE;
    let mut store = create_fresh_store(engine, env_vars, preopened_dirs);
    store.data_mut().runtime_data = Some(runtime_data);
    let mut linker = create_sync_linker(engine)?;
    let (all_instances, target) = instantiate_all_sync(
        compiled_components,
        engine,
        &mut linker,
        &mut store,
        comp_id,
    )
    .map_err(|e| eyre!("{:?}: {}", e.kind, e.message))?;
    store.data_mut().sync_instances = all_instances;
    let func = function
        .get_func(&mut store, &target)
        .map_err(|e| eyre!("{e:#}"))?;
    let mut results = function.new_results_vec();
    func.call(&mut store, inputs, &mut results)
        .map_err(|e| eyre!("{e:#}"))?;
    func.post_return(&mut store).map_err(|e| eyre!("{e:#}"))?;
    Ok(results)
}

/// Fills the `OnceLock` slots for a freshly instantiated component.
pub fn resolve_component_stubs(
    binary: &ComponentBinary,
    instance: &Instance,
    store: &mut impl AsContextMut,
    stubs: &ComponentStubs,
) -> eyre::Result<()> {
    let functions = binary.get_functions();
    for f in functions {
        let Some(inst_name) = f.get_instance_export_name() else {
            continue;
        };
        let key = (inst_name, f.name.name.clone());
        let Some(slot) = stubs.slots.get(&key) else {
            continue;
        };
        let func = f
            .get_func(&mut *store, instance)
            .map_err(|e| eyre!("{e:#?}"))?;
        slot.set(ResolvedFunc {
            func,
            component: binary.component().clone(),
            function_info: f,
        })
        .ok();
    }
    Ok(())
}
