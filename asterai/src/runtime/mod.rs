use crate::plugin::function_name::PluginFunctionName;
use crate::plugin::interface::{PluginFunctionInterface, PluginInterface};
use crate::plugin::{Plugin, PluginId};
use crate::runtime::output::{PluginFunctionOutput, PluginOutput};
use crate::runtime::wasm_instance::PluginRuntimeEngine;
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
pub use wasmtime::component::Val;
use wit_parser::PackageName;

mod entry;
pub mod env;
pub mod output;
mod std_out_err;
mod wasm_instance;
mod wit_bindings;

// The `run/run` function, commonly defined by `wasi:cli`.
static CLI_RUN_FUNCTION_NAME: Lazy<PluginFunctionName> = Lazy::new(|| PluginFunctionName {
    interface: Some("run".to_owned()),
    name: "run".to_owned(),
});

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SerializableVal {
    pub name: Option<String>,
    pub val: Val,
}

/// The runtime which holds all instantiated plugins
/// within an app.
/// TODO: rename to EnvironmentRuntime?
#[derive(Getters)]
pub struct PluginRuntime {
    app_id: Uuid,
    #[getter(skip)]
    engine: PluginRuntimeEngine,
}

impl PluginRuntime {
    pub async fn new(
        plugins: Vec<PluginInterface>,
        // TODO: change app ID for resource ID?
        app_id: Uuid,
        plugin_output_tx: mpsc::Sender<PluginOutput>,
    ) -> eyre::Result<Self> {
        let instance = PluginRuntimeEngine::new(plugins, app_id, plugin_output_tx).await?;
        Ok(Self {
            app_id,
            engine: instance,
        })
    }

    pub fn plugin_interfaces(&self) -> Vec<PluginInterface> {
        self.engine
            .instances()
            .iter()
            .map(|i| i.plugin_interface.clone())
            .collect()
    }

    pub async fn call_function(
        &mut self,
        plugin_manifest_function: PluginFunctionInterface,
        inputs: &[Val],
    ) -> eyre::Result<Option<PluginOutput>> {
        let output_opt = self.engine.call(plugin_manifest_function, inputs).await?;
        Ok(output_opt)
    }

    pub fn find_function(
        &self,
        plugin_id: &PluginId,
        function_name: &PluginFunctionName,
        package_name_opt: Option<PackageName>,
    ) -> Option<PluginFunctionInterface> {
        self.plugin_interfaces().iter().find_map(|interface| {
            if interface.plugin().id() != *plugin_id {
                return None;
            }
            let functions = interface.get_functions();
            let function = functions.into_iter().find(|f| {
                if let Some(package_name) = &package_name_opt {
                    // When specified, ensure the package name matches.
                    if f.package_name != *package_name {
                        return false;
                    }
                }
                &f.name == function_name
            });
            function
        })
    }

    /// Call the `run/run` function, which is commonly defined by `wasi:cli`
    /// to run CLI components.
    /// TODO: call on ALL plugins concurrently. Goal is to use `Func::call_concurrent`.
    pub async fn call_run(&mut self, plugin_id: &PluginId) -> eyre::Result<()> {
        let run_function_opt = self.find_function(
            plugin_id,
            &CLI_RUN_FUNCTION_NAME,
            // Do not specify a package, as usually this is only implemented once.
            // e.g. a common target would be wasi:cli@0.2.0
            None,
        );
        let Some(run_function) = run_function_opt else {
            return Ok(());
        };
        self.call_function(run_function, &[]).await?;
        Ok(())
    }
}

impl PluginOutput {
    pub fn from(
        val_opt: Option<Val>,
        plugin_function_interface: PluginFunctionInterface,
        plugin_response_to_agent_opt: Option<String>,
    ) -> Option<PluginOutput> {
        let function_output_opt = val_opt.and_then(|val| {
            plugin_function_interface
                .clone()
                .output_type
                .map(|type_def| {
                    let name = type_def.name.clone();
                    PluginFunctionOutput {
                        type_def,
                        value: SerializableVal { name, val },
                        function_interface: plugin_function_interface,
                    }
                })
        });
        Some(Self {
            function_output_opt,
            plugin_response_to_agent_opt,
        })
    }
}

impl Debug for PluginRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRuntime")
            .field("app_id", &self.app_id)
            .finish()
    }
}

pub trait PluginFunctionInterfaceExt {
    async fn call(
        self,
        runtime: &mut PluginRuntime,
        inputs: &[Val],
    ) -> eyre::Result<Option<PluginOutput>>;
}

impl PluginFunctionInterfaceExt for PluginFunctionInterface {
    async fn call(
        self,
        runtime: &mut PluginRuntime,
        inputs: &[Val],
    ) -> eyre::Result<Option<PluginOutput>> {
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
