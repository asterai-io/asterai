use crate::plugin::Plugin;
use crate::plugin::function_metadata::PluginFunctionMetadata;
use crate::plugin::function_name::PluginFunctionName;
use derive_getters::Getters;
use eyre::WrapErr;
use eyre::{OptionExt, eyre};
use log::trace;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt::Debug;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;
pub use warg_protocol::registry::PackageName as PackageNameRegistry;
use wasm_pkg_common::label::Label;
use wasm_pkg_common::package::PackageRef;
use wasmtime::component::{
    Component, ComponentNamedList, Func, Instance, Lift, Lower, TypedFunc, Val,
};
use wasmtime::{AsContextMut, Engine};
use wit_bindgen::rt::async_support::futures::StreamExt;
use wit_parser::decoding::DecodedWasm;
use wit_parser::{
    Function, Resolve, Results, Type, TypeDef, TypeDefKind, TypeOwner, World, WorldId, WorldItem,
};

/// A Plugin Manifest represents the metadata of a plugin,
/// including its interface, types, functions and name.
#[derive(Clone)]
pub struct PluginInterface {
    plugin: Plugin,
    /// The Resolve for the component WIT
    /// (not the package/interface WIT).
    /// An important note is that the component/implementation WIT
    /// has its package and world renamed to package root:component
    /// and world name root, therefore the plugin package name
    /// cannot be fetched from this, hence why the `id` field.
    /// The renaming phenomenon happens due to the way WASM tooling works.
    component_wit_resolve: Resolve,
    component_world_id: WorldId,
    component: Arc<Mutex<ComponentBinary>>,
    function_metadata: HashMap<PluginFunctionName, PluginFunctionMetadata>,
}

enum ComponentBinary {
    Raw(Vec<u8>),
    Compiled(Component),
}

#[derive(Getters, Debug, Clone)]
pub struct PluginFunctionInterface {
    pub name: PluginFunctionName,
    /// List of named function inputs and their type defs.
    pub inputs: Vec<(String, TypeDef)>,
    /// A single, optional output is assumed, and that output is not named,
    /// hence only the type is available.
    /// Multiple outputs are not currently supported by WASM/WIT,
    /// although they were initially specified.
    /// Instead, a tuple can be used (which is a single wrapper type).
    pub output_type: Option<TypeDef>,
    pub plugin: Plugin,
}

#[derive(Deserialize, Getters, Debug)]
pub struct PluginManifestFunctionArg {
    name: String,
}

#[derive(Serialize)]
struct SerializablePluginInterface {
    plugin: Plugin,
    functions: Vec<SerializablePluginInterfaceFunction>,
}

#[derive(Serialize)]
struct SerializablePluginInterfaceFunction {
    name: PluginFunctionName,
    inputs: Vec<SerializablePluginInterfaceFunctionInput>,
    output: Option<TypeDefKind>,
}

#[derive(Serialize)]
struct SerializablePluginInterfaceFunctionInput {
    name: String,
    kind: TypeDefKind,
}

impl PluginInterface {
    pub async fn fetch(plugin: Plugin, wkg_client: &wasm_pkg_client::Client) -> eyre::Result<Self> {
        let component_bytes = download_warg_component_package(plugin.clone(), wkg_client).await?;
        let decoded_wasm = wit_parser::decoding::decode(&component_bytes).map_err(|e| eyre!(e))?;
        let (wit_resolve, world_id) = match decoded_wasm {
            DecodedWasm::WitPackage(_, _) => {
                return Err(eyre!("received WIT package instead of component"));
            }
            DecodedWasm::Component(wit_resolve, world_id) => (wit_resolve, world_id),
        };
        let function_metadata = Self::fetch_functions_metadata(&plugin).await?;
        Ok(Self {
            plugin,
            component_wit_resolve: wit_resolve,
            component_world_id: world_id,
            component: Arc::new(Mutex::new(ComponentBinary::Raw(component_bytes))),
            function_metadata,
        })
    }

    async fn fetch_functions_metadata(
        plugin: &Plugin,
    ) -> eyre::Result<HashMap<PluginFunctionName, PluginFunctionMetadata>> {
        // TODO implement.
        todo!()
    }

    /// Fetches a compiled component for this plugin.
    ///
    /// If the component is cached, this will return the cached instance
    /// and return instantly.
    ///
    /// If the instance is not cached, this will compile the component,
    /// store it in this instance's cache and return the component.
    pub async fn fetch_compiled_component(&self, engine: &Engine) -> eyre::Result<Component> {
        let component_cache: &mut ComponentBinary = &mut *self.component.lock().await;
        let uncached_bytes = match component_cache {
            ComponentBinary::Raw(bytes) => bytes,
            ComponentBinary::Compiled(component) => {
                return Ok(component.clone());
            }
        };
        let component =
            Component::from_binary(engine, uncached_bytes.as_slice()).map_err(|e| eyre!(e))?;
        *component_cache = ComponentBinary::Compiled(component.clone());
        Ok(component)
    }

    pub fn plugin(&self) -> &Plugin {
        &self.plugin
    }

    /// A stringified version of the interface, including all agentic functions.
    pub fn stringified_agentic_interface(&self) -> String {
        self.stringified_interface(true)
    }

    /// A stringified version of the interface, optional including only agentic functions.
    pub fn stringified_interface(&self, only_agentic: bool) -> String {
        // TODO: implement fully
        let mut string = String::new();
        let functions = self.get_functions();
        for function in functions {
            let is_agentic = self.get_function_metadata(&function.name).is_agentic;
            if !is_agentic && only_agentic {
                continue;
            }
            string.push_str(&format!(
                "function name {} of manifest '{}' inputs: (",
                function.name.name,
                self.plugin.id()
            ));
            for (name, type_def) in function.inputs {
                string.push_str(&format!("{}: {:#?},", name, type_def_to_string(&type_def)));
            }
            string.push_str(") output type: ");
            string.push_str(
                &function
                    .output_type
                    .map(|t| type_def_to_string(&t))
                    .unwrap_or_else(|| "function has no output".to_owned()),
            );
            string.push_str("\n");
        }
        string
    }

    pub fn get_functions(&self) -> Vec<PluginFunctionInterface> {
        let world = self.component_world();
        world
            .exports
            .iter()
            .flat_map(|(_, item)| match item {
                WorldItem::Interface { id, .. } => {
                    let interface = self.component_wit_resolve.interfaces.get(*id).unwrap();
                    let interface_name = interface.name.clone().unwrap_or_else(|| String::new());
                    interface
                        .functions
                        .iter()
                        .map(|(_, function)| {
                            self.map_wit_function_plugin_function(
                                function,
                                Some(interface_name.clone()),
                            )
                        })
                        .collect()
                }
                WorldItem::Function(function) => {
                    vec![self.map_wit_function_plugin_function(function, None)]
                }
                WorldItem::Type(_) => Vec::new(),
            })
            .collect()
    }

    pub fn get_imports_count(&self) -> usize {
        let world = self.component_world();
        world.imports.len()
    }

    pub fn get_function_metadata(
        &self,
        function_name: &PluginFunctionName,
    ) -> PluginFunctionMetadata {
        self.function_metadata
            .get(&function_name)
            .cloned()
            .unwrap_or_default()
    }

    fn component_world(&self) -> &World {
        let world = self
            .component_wit_resolve
            .worlds
            .get(self.component_world_id)
            .unwrap();
        world
    }

    fn map_wit_function_plugin_function(
        &self,
        function: &Function,
        interface_name: Option<String>,
    ) -> PluginFunctionInterface {
        let output_type = {
            if function.results.len() == 0 {
                None
            } else {
                let result_type = match function.results {
                    Results::Named(_) => {
                        panic!("multiple, named function return values not supported currently");
                    }
                    Results::Anon(result_type) => result_type,
                };
                Some(self.map_type_to_type_def(result_type))
            }
        };
        let input_types = function
            .params
            .clone()
            .into_iter()
            .map(|(name, wit_type)| (name, self.map_type_to_type_def(wit_type)))
            .collect();
        PluginFunctionInterface {
            name: PluginFunctionName::new(interface_name, function.name.clone()),
            inputs: input_types,
            output_type,
            plugin: self.plugin.clone(),
        }
    }

    fn map_type_to_type_def(&self, wit_type: Type) -> TypeDef {
        match wit_type {
            Type::Id(type_id) => self
                .component_wit_resolve
                .types
                .get(type_id)
                .unwrap()
                .to_owned(),
            _ => TypeDef {
                name: None,
                kind: TypeDefKind::Type(wit_type),
                owner: TypeOwner::None,
                docs: Default::default(),
                stability: Default::default(),
            },
        }
    }
}

impl PluginFunctionInterface {
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
        let version_string = self.plugin.version().to_string();
        // Export name example: asterai:hello/greet@0.2.0
        let export_name = format!("{}/{}@{version_string}", self.plugin.id(), interface_name);
        trace!("interface export name: {}", export_name);
        let interface_export = instance
            .get_export(&mut store, None, &export_name)
            .ok_or_eyre(eyre!("interface export not found"))?;
        trace!("function export name: {}", &self.name);
        let func_export = instance
            .get_export(&mut store, Some(&interface_export), &self.name.name)
            .ok_or_eyre(eyre!("function export not found"))?;
        let func = instance
            .get_func(&mut store, &func_export)
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
        let version_string = self.plugin.version().to_string();
        // Export name example: asterai:hello/greet@0.2.0
        let export_name = format!("{}/{}@{version_string}", self.plugin.id(), interface_name);
        trace!("interface export name: {}", export_name);
        let interface_export = instance
            .get_export(&mut store, None, &export_name)
            .ok_or_eyre(eyre!("interface export not found"))?;
        trace!("function export name: {}", &self.name);
        let func_export = instance
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
        let version_string = format!("@{}", self.plugin.version());
        let export_name = format!("{}/{}{}", self.plugin.id(), interface_name, version_string);
        Some(export_name)
    }
}

impl Debug for PluginInterface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginInterface")
            .field("plugin", &self.plugin)
            .field("component_wit_resolve", &self.component_wit_resolve)
            .field("component_world_id", &self.component_world_id)
            .finish()
    }
}

async fn download_warg_component_package(
    plugin: Plugin,
    wkg_client: &wasm_pkg_client::Client,
) -> eyre::Result<Vec<u8>> {
    let package_name = format!("{}-component", plugin.name());
    let package = PackageRef::new(
        Label::from_str(plugin.namespace())?,
        Label::from_str(&package_name)?,
    );
    let release = wkg_client.get_release(&package, plugin.version()).await?;
    let mut content_stream = wkg_client.stream_content(&package, &release).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = content_stream.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

fn type_def_to_string(type_def: &TypeDef) -> String {
    match &type_def.kind {
        TypeDefKind::Record(r) => {
            format!("struct: {:#?}", r)
        }
        _ => {
            format!("{:#?}", type_def.kind)
        }
    }
}

impl Serialize for PluginInterface {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serializable: SerializablePluginInterface = self.into();
        serializable.serialize(serializer)
    }
}

impl From<&PluginInterface> for SerializablePluginInterface {
    fn from(v: &PluginInterface) -> Self {
        let functions = v
            .get_functions()
            .into_iter()
            .map(|f| SerializablePluginInterfaceFunction {
                name: f.name,
                inputs: f
                    .inputs
                    .into_iter()
                    .map(
                        |(name, type_def)| SerializablePluginInterfaceFunctionInput {
                            name,
                            kind: type_def.kind,
                        },
                    )
                    .collect(),
                output: f.output_type.map(|type_def| type_def.kind),
            })
            .collect();
        Self {
            plugin: v.plugin.clone(),
            functions,
        }
    }
}
