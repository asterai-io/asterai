use crate::component::Component;
use crate::component::function_name::ComponentFunctionName;
use derive_getters::Getters;
use eyre::{OptionExt, WrapErr, eyre};
use log::trace;
use wasmtime::AsContextMut;
use wasmtime::component::{ComponentNamedList, Func, Instance, Lift, Lower, TypedFunc, Val};
use wit_parser::{PackageName, TypeDef};

#[derive(Getters, Debug, Clone)]
pub struct ComponentFunctionInterface {
    /// Package name where the function signature is defined.
    ///
    /// The package may be the component's own package, e.g. user:my-component or
    /// an external package, e.g. wasi:cli if the component implements an external
    /// package's interface, such as wasi:cli's run function for WASI CLI binaries.
    pub package_name: PackageName,
    pub name: ComponentFunctionName,
    /// List of named function inputs and their type defs.
    pub inputs: Vec<(String, TypeDef)>,
    /// A single, optional output is assumed, and that output is not named,
    /// hence only the type is available.
    /// Multiple outputs are not currently supported by WASM/WIT,
    /// although they were initially specified.
    /// Instead, a tuple can be used (which is a single wrapper type).
    pub output_type: Option<TypeDef>,
    /// What component this function belongs to,
    /// i.e. this includes the package name where the function is implemented.
    pub component: Component,
}

impl ComponentFunctionInterface {
    pub fn new_results_vec(&self) -> Vec<Val> {
        match self.output_type {
            None => Vec::new(),
            // This will be overridden, so it can be any value here.
            Some(_) => vec![Val::Bool(false)],
        }
    }

    pub fn get_func(
        &self,
        mut store: impl AsContextMut,
        instance: &Instance,
    ) -> eyre::Result<Func> {
        let Some(interface_name) = &self.name.interface else {
            // This function is not exported from an interface.
            let func = instance
                .get_func(&mut store, &self.name.name)
                .ok_or_eyre(eyre!("function not found"))?;
            return Ok(func);
        };
        let version_string = self
            .package_name
            .version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_default();
        let package_name = format!("{}:{}", self.package_name.namespace, self.package_name.name);
        // Export name example: asterai:hello/greet@0.2.0
        let export_name = format!("{package_name}/{interface_name}@{version_string}");
        trace!("interface export name: {}", export_name);
        let (_, interface_export) = instance
            .get_export(&mut store, None, &export_name)
            .ok_or_eyre(eyre!("interface export '{export_name}' not found"))?;
        trace!("function export name: {}", &self.name);
        let (_, func_export) = instance
            .get_export(&mut store, Some(&interface_export), &self.name.name)
            .ok_or_eyre(eyre!(
                "function export '{export_name}/{}' not found",
                self.name.name
            ))?;
        let func = instance
            .get_func(&mut store, func_export)
            .ok_or_eyre(eyre!("function not found"))?;
        Ok(func)
    }

    pub fn get_typed_func<Params, Result>(
        &self,
        mut store: impl AsContextMut,
        instance: &Instance,
    ) -> eyre::Result<TypedFunc<Params, Result>>
    where
        Params: ComponentNamedList + Lower,
        Result: ComponentNamedList + Lift,
    {
        let Some(interface_name) = &self.name.interface else {
            // This function is not exported from an interface.
            let func = instance
                .get_typed_func::<Params, Result>(&mut store, &self.name.name)
                .map_err(|e| eyre!(e))
                .with_context(|| "(typed) function not found")?;
            return Ok(func);
        };
        let version_string = self.component.version().to_string();
        // Export name example: asterai:hello/greet@0.2.0
        let export_name = format!(
            "{}/{}@{version_string}",
            self.component.id(),
            interface_name
        );
        trace!("interface export name: {}", export_name);
        let (_, interface_export) = instance
            .get_export(&mut store, None, &export_name)
            .ok_or_eyre(eyre!("interface export not found"))?;
        trace!("function export name: {}", &self.name);
        let (_, func_export) = instance
            .get_export(&mut store, Some(&interface_export), &self.name.name)
            .ok_or_eyre(eyre!("function export not found"))?;
        let func = instance
            .get_typed_func::<Params, Result>(&mut store, &func_export)
            .map_err(|e| eyre!(e))
            .with_context(|| "(typed) function not found")?;
        Ok(func)
    }

    /// Gets the export name for this function within the linker.
    /// If this is a world root function, then this is None
    /// as the function is available through the "root" instance
    /// itself via `get_func` and does not need to be added
    /// to the linker.
    pub fn get_instance_export_name(&self) -> Option<String> {
        let Some(interface_name) = &self.name.interface else {
            return None;
        };
        let version_string = format!("@{}", self.component.version());
        let export_name = format!(
            "{}/{}{}",
            self.component.id(),
            interface_name,
            version_string
        );
        Some(export_name)
    }
}
