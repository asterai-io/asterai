use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use crate::runtime::build_runtime;
use asterai_runtime::component::function_name::ComponentFunctionName;
use asterai_runtime::component::{PackageName, Version};
use asterai_runtime::runtime::Val;
use eyre::{OptionExt, bail};
use std::str::FromStr;

impl EnvArgs {
    pub async fn call(&self) -> eyre::Result<()> {
        let resource = self.resource()?;
        let component = self.component.as_ref().unwrap();
        let function_string = self.function.clone().unwrap();
        println!("calling env {resource}'s {component} component function {function_string}");
        let environment = LocalStore::fetch_environment(&resource.id())?;
        let mut runtime = build_runtime(environment).await?;
        let (function_name, package_name_opt) = parse_function_string_into_parts(function_string)?;
        let function = runtime
            .find_function(&component.id(), &function_name, package_name_opt)
            .ok_or_eyre("function not found")?;
        let inputs = parse_inputs_from_string_args(&self.function_args)?;
        runtime.call_function(function, &inputs).await?;
        Ok(())
    }
}

fn parse_function_string_into_parts(
    function_str: String,
) -> eyre::Result<(ComponentFunctionName, Option<PackageName>)> {
    let (package_name_str, function) = match function_str.split_once('/') {
        None => {
            return Ok((
                ComponentFunctionName {
                    interface: None,
                    name: function_str,
                },
                None,
            ));
        }
        Some((a, b)) => {
            if !a.contains(":") {
                return Ok((
                    ComponentFunctionName {
                        interface: Some(a.to_owned()),
                        name: b.to_owned(),
                    },
                    None,
                ));
            }
            (a, b)
        }
    };
    let package_name = package_name_from_str(&package_name_str)?;
    let (function_name, interface_opt) = match function.split_once('/') {
        None => (function, None),
        Some((a, b)) => (a, Some(b)),
    };
    Ok((
        ComponentFunctionName {
            interface: interface_opt.map(|v| v.to_owned()),
            name: function_name.to_owned(),
        },
        Some(package_name),
    ))
}

fn parse_inputs_from_string_args(_args: &[String]) -> eyre::Result<Vec<Val>> {
    // TODO
    Ok(Vec::new())
}

fn package_name_from_str(package_name_str: &str) -> eyre::Result<PackageName> {
    let (id, version_str_opt) = match package_name_str.split_once('@') {
        None => (package_name_str, None),
        Some((a, b)) => (a, Some(b)),
    };
    let (namespace, name) = match id.split_once(':') {
        None => bail!("invalid package name"),
        Some((a, b)) => (a, b),
    };
    let version_opt = match version_str_opt {
        Some(v) => Some(Version::from_str(v)?),
        None => None,
    };
    Ok(PackageName {
        namespace: namespace.to_owned(),
        name: name.to_owned(),
        version: version_opt,
    })
}
