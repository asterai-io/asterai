use crate::component::Component;
use crate::component::function_interface::ComponentFunctionInterface;
use crate::component::function_name::ComponentFunctionName;
use crate::component::wit::{
    ComponentFunction, ComponentInterface, ComponentWit, ExportedInterface, ImportedInterface,
};
use derive_getters::Getters;
use eyre::eyre;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::Debug;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;
pub use warg_protocol::registry::PackageName as PackageNameRegistry;
use wasm_pkg_common::label::Label;
use wasm_pkg_common::package::PackageRef;
use wasmtime::Engine;
pub use wasmtime::component::Component as WasmtimeComponent;
use wit_bindgen::rt::async_support::futures::StreamExt;
use wit_parser::decoding::DecodedWasm;
use wit_parser::{
    Function, PackageName, Results, Type, TypeDef, TypeDefKind, TypeOwner, WorldItem,
};

/// A component with its fully resolved interface
/// as well as the compiled binary.
#[derive(Clone)]
pub struct ComponentBinary {
    component: Component,
    /// The component/implementation WIT has its package and world
    /// renamed to package root:component and world name root,
    /// therefore the component package name cannot be fetched from
    /// this, hence why the `component` field exists separately.
    /// The renaming phenomenon happens due to the way WASM tooling works.
    wit: ComponentWit,
    wasmtime_component: Arc<Mutex<WasmtimeComponentBinary>>,
}

enum WasmtimeComponentBinary {
    Raw(Vec<u8>),
    Compiled(WasmtimeComponent),
}

#[derive(Deserialize, Getters, Debug)]
pub struct ComponentManifestFunctionArg {
    name: String,
}

#[derive(Serialize)]
struct SerializableComponentInterface {
    component: Component,
    functions: Vec<SerializableComponentInterfaceFunction>,
}

#[derive(Serialize)]
struct SerializableComponentInterfaceFunction {
    name: ComponentFunctionName,
    inputs: Vec<SerializableComponentInterfaceFunctionInput>,
    output: Option<TypeDefKind>,
}

#[derive(Serialize)]
struct SerializableComponentInterfaceFunctionInput {
    name: String,
    kind: TypeDefKind,
}

impl ComponentBinary {
    pub async fn fetch(
        component: Component,
        wkg_client: &wasm_pkg_client::Client,
    ) -> eyre::Result<Self> {
        let component_bytes =
            download_component_package_from_registry(component.clone(), wkg_client).await?;
        Self::from_component_bytes(component, component_bytes)
    }

    pub fn from_component_bytes(
        component: Component,
        component_bytes: Vec<u8>,
    ) -> eyre::Result<Self> {
        let decoded_wasm = wit_parser::decoding::decode(&component_bytes).map_err(|e| eyre!(e))?;
        let (resolve, world_id) = match decoded_wasm {
            DecodedWasm::WitPackage(_, _) => {
                return Err(eyre!("received WIT package instead of component"));
            }
            DecodedWasm::Component(resolve, world_id) => (resolve, world_id),
        };
        Ok(Self {
            component,
            wit: ComponentWit::new(resolve, world_id),
            wasmtime_component: Arc::new(Mutex::new(WasmtimeComponentBinary::Raw(component_bytes))),
        })
    }

    /// Fetches a compiled component for this component.
    ///
    /// If the component is cached, this will return the cached instance
    /// and return instantly.
    ///
    /// If the instance is not cached, this will compile the component,
    /// store it in this instance's cache and return the component.
    pub async fn fetch_compiled_component(
        &self,
        engine: &Engine,
    ) -> eyre::Result<WasmtimeComponent> {
        let component_cache: &mut WasmtimeComponentBinary =
            &mut *self.wasmtime_component.lock().await;
        let uncached_bytes = match component_cache {
            WasmtimeComponentBinary::Raw(bytes) => bytes,
            WasmtimeComponentBinary::Compiled(component) => {
                return Ok(component.clone());
            }
        };
        let component = WasmtimeComponent::from_binary(engine, uncached_bytes.as_slice())
            .map_err(|e| eyre!(e))?;
        *component_cache = WasmtimeComponentBinary::Compiled(component.clone());
        Ok(component)
    }

    pub fn component(&self) -> &Component {
        &self.component
    }

    /// Human-readable summary of the component's exported interfaces
    /// and world-level functions.
    // TODO: interface names here use `root:component` instead of the
    // real package name due to WASM tooling renaming (see `wit` field docs).
    pub fn stringified_interface(&self) -> String {
        let mut out = String::new();
        let interfaces = self.exported_interfaces();
        if !interfaces.is_empty() {
            out.push_str("Exports:\n");
            for interface in &interfaces {
                out.push_str(&format!("  {}\n", interface.name));
                for f in &interface.functions {
                    out.push_str(&format!("    {}\n", format_function_signature(f)));
                }
            }
        }
        let world_fns = self.world_functions();
        if !world_fns.is_empty() {
            out.push_str("Non-composable entry functions:\n");
            for f in &world_fns {
                out.push_str(&format!("  {}\n", format_function_signature(f)));
            }
        }
        out
    }

    pub fn wit(&self) -> &ComponentWit {
        &self.wit
    }

    pub fn get_functions(&self) -> Vec<ComponentFunctionInterface> {
        let resolve = self.wit.resolve();
        let world = self.wit.world();
        let component_package_name = self.component.package_name();
        world
            .exports
            .iter()
            .flat_map(|(_, item)| match item {
                WorldItem::Interface { id, .. } => {
                    let interface = resolve.interfaces.get(*id).unwrap();
                    let package_name = match interface.package {
                        None => component_package_name,
                        Some(package_id) => {
                            let package_opt = resolve.packages.get(package_id);
                            package_opt
                                .map(|p| &p.name)
                                .unwrap_or(component_package_name)
                        }
                    };
                    let interface_name = interface.name.clone().unwrap_or_default();
                    interface
                        .functions
                        .iter()
                        .map(|(_, function)| {
                            self.map_wit_function_component_function(
                                function,
                                Some(interface_name.clone()),
                                package_name.clone(),
                            )
                        })
                        .collect()
                }
                WorldItem::Function(function) => {
                    let package_name = self.component.package_name.clone();
                    vec![self.map_wit_function_component_function(function, None, package_name)]
                }
                WorldItem::Type(_) => Vec::new(),
            })
            .collect()
    }

    pub fn get_imports_count(&self) -> usize {
        self.wit.world().imports.len()
    }

    fn map_wit_function_component_function(
        &self,
        function: &Function,
        interface_name: Option<String>,
        package_name: PackageName,
    ) -> ComponentFunctionInterface {
        let resolve = self.wit.resolve();
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
                Some(map_type_to_type_def(resolve, result_type))
            }
        };
        let input_types = function
            .params
            .clone()
            .into_iter()
            .map(|(name, wit_type)| (name, map_type_to_type_def(resolve, wit_type)))
            .collect();
        ComponentFunctionInterface {
            package_name,
            name: ComponentFunctionName::new(interface_name, function.name.clone()),
            inputs: input_types,
            output_type,
            component: self.component.clone(),
        }
    }
}

impl ComponentInterface for ComponentBinary {
    fn imported_interfaces(&self) -> Vec<ImportedInterface> {
        self.wit.imported_interfaces()
    }

    fn exported_interfaces(&self) -> Vec<ExportedInterface> {
        self.wit.exported_interfaces()
    }

    fn world_functions(&self) -> Vec<ComponentFunction> {
        self.wit.world_functions()
    }
}

impl Debug for ComponentBinary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentBinary")
            .field("component", &self.component)
            .finish()
    }
}

fn map_type_to_type_def(resolve: &wit_parser::Resolve, wit_type: Type) -> TypeDef {
    match wit_type {
        Type::Id(type_id) => resolve.types.get(type_id).unwrap().to_owned(),
        _ => TypeDef {
            name: None,
            kind: TypeDefKind::Type(wit_type),
            owner: TypeOwner::None,
            docs: Default::default(),
            stability: Default::default(),
        },
    }
}

async fn download_component_package_from_registry(
    component: Component,
    wkg_client: &wasm_pkg_client::Client,
) -> eyre::Result<Vec<u8>> {
    let package_name = format!("{}-component", component.name());
    let package = PackageRef::new(
        Label::from_str(component.namespace())?,
        Label::from_str(&package_name)?,
    );
    let release = wkg_client
        .get_release(&package, component.version())
        .await?;
    let mut content_stream = wkg_client.stream_content(&package, &release).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = content_stream.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

fn format_function_signature(f: &ComponentFunction) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.type_name))
        .collect();
    match &f.return_type {
        Some(ret) => format!("{}({}) -> {}", f.name, params.join(", "), ret),
        None => format!("{}({})", f.name, params.join(", ")),
    }
}

impl Serialize for ComponentBinary {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serializable: SerializableComponentInterface = self.into();
        serializable.serialize(serializer)
    }
}

impl From<&ComponentBinary> for SerializableComponentInterface {
    fn from(v: &ComponentBinary) -> Self {
        let functions = v
            .get_functions()
            .into_iter()
            .map(|f| SerializableComponentInterfaceFunction {
                name: f.name,
                inputs: f
                    .inputs
                    .into_iter()
                    .map(
                        |(name, type_def)| SerializableComponentInterfaceFunctionInput {
                            name,
                            kind: type_def.kind,
                        },
                    )
                    .collect(),
                output: f.output_type.map(|type_def| type_def.kind),
            })
            .collect();
        Self {
            component: v.component.clone(),
            functions,
        }
    }
}
