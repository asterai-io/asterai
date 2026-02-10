//! Pre-registers forwarding stubs in the linker for component-to-component
//! exports, allowing instantiation in any order (including cycles).
use crate::component::Component;
use crate::component::binary::ComponentBinary;
use crate::component::function_interface::ComponentFunctionInterface;
use crate::runtime::env::HostEnv;
use crate::runtime::wasm_instance::{call_wasm_component_function, parse_component_output};
use eyre::eyre;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use wasmtime::component::{Func, Instance, Linker};
use wasmtime::{AsContext, AsContextMut};

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
/// exported by any component. Each stub delegates to an
/// `Arc<OnceLock<ResolvedFunc>>` that gets filled after instantiation.
pub fn register_component_stubs(
    components: &[ComponentBinary],
    linker: &mut Linker<HostEnv>,
) -> eyre::Result<ComponentStubs> {
    let mut slots: HashMap<SlotKey, FuncSlot> = HashMap::new();
    for comp in components {
        let functions = comp.get_functions();
        let mut by_instance: HashMap<String, Vec<ComponentFunctionInterface>> = HashMap::new();
        for f in functions {
            let Some(inst) = f.get_instance_export_name() else {
                continue;
            };
            by_instance.entry(inst).or_default().push(f);
        }
        for (inst_name, funcs) in by_instance {
            let mut inst_builder = linker.instance(&inst_name).map_err(|e| eyre!("{e:#?}"))?;
            for f in funcs {
                let key = (inst_name.clone(), f.name.name.clone());
                let slot: FuncSlot = Arc::new(OnceLock::new());
                let slot_ref = slot.clone();
                inst_builder
                    .func_new_async(&f.name.name, move |mut store, _, params, results| {
                        let slot = slot_ref.clone();
                        Box::new(async move {
                            let resolved = slot.get().ok_or_else(|| {
                                wasmtime::Error::msg("unresolved component function")
                            })?;
                            call_wasm_component_function(
                                &resolved.func,
                                &resolved.function_info.name,
                                store.as_context_mut(),
                                params,
                                results,
                                resolved.component.clone(),
                            )
                            .await
                            .unwrap();
                            let output_opt = parse_component_output(
                                store.as_context(),
                                results.to_vec(),
                                resolved.function_info.clone(),
                            );
                            if let Some(output) = output_opt {
                                store.data().component_output_tx.send(output).await.unwrap();
                            }
                            Ok(())
                        })
                    })
                    .map_err(|e| eyre!("{e:#?}"))?;
                slots.insert(key, slot);
            }
        }
    }
    Ok(ComponentStubs { slots })
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
