use crate::component::function_name::ComponentFunctionName;
use crate::component::interface::{ComponentFunctionInterface, ComponentInterface};
use crate::component::{Component, ComponentId};
use crate::runtime::output::{ComponentFunctionOutput, ComponentOutput};
use crate::runtime::wasm_instance::{
    ComponentRuntimeEngine, call_wasm_component_function_concurrent,
};
use derive_getters::Getters;
use eyre::eyre;
use futures::future::{join_all, try_join_all};
use log::error;
use once_cell::sync::Lazy;
use semver::Version;
use serde_json::Value;
use std::fmt::Debug;
use tokio::sync::mpsc;
use uuid::Uuid;
use wasmtime::AsContextMut;
pub use wasmtime::component::Val;
use wit_parser::PackageName;

mod entry;
pub mod env;
pub mod output;
mod std_out_err;
mod wasm_instance;
mod wit_bindings;

// The `run/run` function, commonly defined by `wasi:cli`.
static CLI_RUN_FUNCTION_NAME: Lazy<ComponentFunctionName> = Lazy::new(|| ComponentFunctionName {
    interface: Some("run".to_owned()),
    name: "run".to_owned(),
});

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SerializableVal {
    pub name: Option<String>,
    pub val: Val,
}

/// The runtime which holds all instantiated components
/// within an app.
/// TODO: rename to EnvironmentRuntime?
#[derive(Getters)]
pub struct ComponentRuntime {
    app_id: Uuid,
    #[getter(skip)]
    engine: ComponentRuntimeEngine,
}

impl ComponentRuntime {
    pub async fn new(
        components: Vec<ComponentInterface>,
        // TODO: change app ID for resource ID?
        app_id: Uuid,
        component_output_tx: mpsc::Sender<ComponentOutput>,
    ) -> eyre::Result<Self> {
        let instance = ComponentRuntimeEngine::new(components, app_id, component_output_tx).await?;
        Ok(Self {
            app_id,
            engine: instance,
        })
    }

    pub fn component_interfaces(&self) -> Vec<ComponentInterface> {
        self.engine
            .instances()
            .iter()
            .map(|i| i.component_interface.clone())
            .collect()
    }

    pub async fn call_function(
        &mut self,
        component_manifest_function: ComponentFunctionInterface,
        inputs: &[Val],
    ) -> eyre::Result<Option<ComponentOutput>> {
        let output_opt = self.engine.call(component_manifest_function, inputs).await?;
        Ok(output_opt)
    }

    pub fn find_function(
        &self,
        component_id: &ComponentId,
        function_name: &ComponentFunctionName,
        package_name_opt: Option<PackageName>,
    ) -> Option<ComponentFunctionInterface> {
        self.component_interfaces().iter().find_map(|interface| {
            if interface.component().id() != *component_id {
                return None;
            }
            let functions = interface.get_functions();
            let function = functions.into_iter().find(|f| {
                if let Some(package_name) = &package_name_opt {
                    let is_same_package = f.package_name.name == package_name.name
                        && f.package_name.namespace == package_name.namespace;
                    // When specified, ensure the package name matches.
                    if !is_same_package {
                        return false;
                    }
                    let is_compatible_version = package_name.version.is_none()
                        || package_name.version == f.package_name.version;
                    if !is_compatible_version {
                        return false;
                    }
                }
                &f.name == function_name
            });
            function
        })
    }

    /// Call all the `run` functions concurrently, which is commonly defined by `wasi:cli/run`
    /// to run CLI components, on all components that implement it.
    pub async fn run(&mut self) -> eyre::Result<()> {
        let mut funcs = Vec::new();
        for instance in &self.engine.instances {
            let component = instance.component_interface.component().clone();
            let run_function_opt = self.find_function(
                &component.id(),
                &CLI_RUN_FUNCTION_NAME,
                // Do not specify a package, as usually this is only implemented once.
                // e.g. a common target would be wasi:cli@0.2.0
                None,
            );
            let Some(run_function) = run_function_opt else {
                return Ok(());
            };
            let func = run_function.get_func(&mut self.engine.store, &instance.instance)?;
            funcs.push((func, run_function.name, component));
        }
        self.engine
            .store
            .run_concurrent(async |a| {
                for (func, func_name, component) in funcs {
                    let result = call_wasm_component_function_concurrent(
                        &func,
                        &func_name,
                        a,
                        &[],
                        &mut vec![Val::Bool(false)],
                        component,
                    )
                    .await;
                    if let Err(e) = result {
                        error!("{e:#?}");
                    }
                }
            })
            .await
            .map_err(|e| eyre!(e))?;
        Ok(())
    }
}

impl ComponentOutput {
    pub fn from(
        val_opt: Option<Val>,
        component_function_interface: ComponentFunctionInterface,
        component_response_to_agent_opt: Option<String>,
    ) -> Option<ComponentOutput> {
        let function_output_opt = val_opt.and_then(|val| {
            component_function_interface
                .clone()
                .output_type
                .map(|type_def| {
                    let name = type_def.name.clone();
                    ComponentFunctionOutput {
                        type_def,
                        value: SerializableVal { name, val },
                        function_interface: component_function_interface,
                    }
                })
        });
        Some(Self {
            function_output_opt,
            component_response_to_agent_opt,
        })
    }
}

impl Debug for ComponentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentRuntime")
            .field("app_id", &self.app_id)
            .finish()
    }
}

pub trait ComponentFunctionInterfaceExt {
    async fn call(
        self,
        runtime: &mut ComponentRuntime,
        inputs: &[Val],
    ) -> eyre::Result<Option<ComponentOutput>>;
}

impl ComponentFunctionInterfaceExt for ComponentFunctionInterface {
    async fn call(
        self,
        runtime: &mut ComponentRuntime,
        inputs: &[Val],
    ) -> eyre::Result<Option<ComponentOutput>> {
        runtime.call_function(self, inputs).await
    }
}

pub trait ValExt {
    fn try_into_json_value(self) -> Option<Value>;
}

impl ValExt for Val {
    fn try_into_json_value(self) -> Option<Value> {
        let value = match self {
            Val::Bool(v) => Value::Bool(v),
            Val::S8(v) => Value::Number(v.into()),
            Val::U8(v) => Value::Number(v.into()),
            Val::S16(v) => Value::Number(v.into()),
            Val::U16(v) => Value::Number(v.into()),
            Val::S32(v) => Value::Number(v.into()),
            Val::U32(v) => Value::Number(v.into()),
            Val::S64(v) => Value::Number(v.into()),
            Val::U64(v) => Value::Number(v.into()),
            Val::Float32(v) => Value::Number(serde_json::Number::from_f64(v as f64).unwrap()),
            Val::Float64(v) => Value::Number(serde_json::Number::from_f64(v).unwrap()),
            Val::Char(v) => Value::String(v.to_string()),
            Val::String(v) => Value::String(v),
            Val::List(v) => Value::Array(
                v.iter()
                    .filter_map(|val| val.clone().try_into_json_value())
                    .collect(),
            ),
            Val::Tuple(v) => Value::Array(
                v.iter()
                    .filter_map(|val| val.clone().try_into_json_value())
                    .collect(),
            ),
            Val::Option(v) => v
                .as_deref()
                .and_then(|v| v.clone().try_into_json_value())
                .unwrap_or(Value::Null),
            Val::Result(v) => v
                .ok()
                .and_then(|v| v.clone()?.try_into_json_value())
                .unwrap_or(Value::Null),
            Val::Variant(_, _) => return None,
            Val::Enum(_) => return None,
            Val::Record(_) => todo!(),
            Val::Flags(_) => return None,
            Val::Resource(_) => return None,
            Val::Future(_) => return None,
            Val::Stream(_) => return None,
            Val::ErrorContext(_) => return None,
        };
        Some(value)
    }
}
