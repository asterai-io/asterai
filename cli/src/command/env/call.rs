use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use crate::runtime::build_runtime;
use asterai_runtime::component::function_name::ComponentFunctionName;
use asterai_runtime::component::{PackageName, Version};
use asterai_runtime::runtime::Val;
use asterai_runtime::runtime::parsing::{ValExt, json_value_to_val, parse_primitive};
use eyre::{OptionExt, bail};
use std::str::FromStr;
use wit_parser::{TypeDef, TypeDefKind};

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
        let inputs = parse_inputs_from_string_args(&self.function_args, &function.inputs)?;
        let output_opt = runtime.call_function(function, &inputs).await?;
        if let Some(output) = output_opt
            && let Some(function_output) = output.function_output_opt
        {
            print_val(function_output.value.val);
        }
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
    let package_name = package_name_from_str(package_name_str)?;
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

fn parse_inputs_from_string_args(
    args: &[String],
    expected_inputs: &[(String, TypeDef)],
) -> eyre::Result<Vec<Val>> {
    if args.len() != expected_inputs.len() {
        bail!(
            "expected {} argument(s), got {}",
            expected_inputs.len(),
            args.len()
        );
    }
    args.iter()
        .zip(expected_inputs.iter())
        .map(|(arg, (name, type_def))| {
            parse_arg(arg, type_def)
                .map_err(|e| eyre::eyre!("failed to parse argument '{name}': {e}"))
        })
        .collect()
}

fn parse_arg(arg: &str, type_def: &TypeDef) -> eyre::Result<Val> {
    match &type_def.kind {
        TypeDefKind::Type(ty) => parse_primitive(strip_quotes(arg), ty),
        TypeDefKind::Record(record) => {
            let json: serde_json::Value = serde_json::from_str(arg)
                .map_err(|e| eyre::eyre!("expected JSON for record: {e}"))?;
            let serde_json::Value::Object(map) = json else {
                bail!("expected JSON object for record");
            };
            let fields = record
                .fields
                .iter()
                .map(|field| {
                    let value = map
                        .get(&field.name)
                        .ok_or_else(|| eyre::eyre!("missing field '{}'", field.name))?;
                    let val = json_value_to_val(value, &field.ty)?;
                    Ok((field.name.clone(), val))
                })
                .collect::<eyre::Result<Vec<_>>>()?;
            Ok(Val::Record(fields))
        }
        TypeDefKind::List(ty) => {
            let json: serde_json::Value = serde_json::from_str(arg)
                .map_err(|e| eyre::eyre!("expected JSON for list: {e}"))?;
            let serde_json::Value::Array(arr) = json else {
                bail!("expected JSON array for list");
            };
            let vals = arr
                .iter()
                .map(|v| json_value_to_val(v, ty))
                .collect::<eyre::Result<Vec<_>>>()?;
            Ok(Val::List(vals))
        }
        TypeDefKind::Tuple(tuple) => {
            let json: serde_json::Value = serde_json::from_str(arg)
                .map_err(|e| eyre::eyre!("expected JSON for tuple: {e}"))?;
            let serde_json::Value::Array(arr) = json else {
                bail!("expected JSON array for tuple");
            };
            if arr.len() != tuple.types.len() {
                bail!(
                    "tuple has {} elements, got {}",
                    tuple.types.len(),
                    arr.len()
                );
            }
            let vals = arr
                .iter()
                .zip(tuple.types.iter())
                .map(|(v, ty)| json_value_to_val(v, ty))
                .collect::<eyre::Result<Vec<_>>>()?;
            Ok(Val::Tuple(vals))
        }
        TypeDefKind::Enum(e) => {
            let arg = strip_quotes(arg);
            let is_valid = e.cases.iter().any(|c| c.name == arg);
            if !is_valid {
                let cases: Vec<&str> = e.cases.iter().map(|c| c.name.as_str()).collect();
                bail!("invalid enum value '{arg}', expected one of: {cases:?}");
            }
            Ok(Val::Enum(arg.to_owned()))
        }
        TypeDefKind::Option(ty) => {
            let arg_stripped = strip_quotes(arg);
            if arg_stripped == "null" || arg_stripped == "none" {
                return Ok(Val::Option(None));
            }
            let inner = json_value_to_val(&serde_json::from_str(arg)?, ty)?;
            Ok(Val::Option(Some(Box::new(inner))))
        }
        TypeDefKind::Flags(flags) => {
            let json: serde_json::Value = serde_json::from_str(arg)
                .map_err(|e| eyre::eyre!("expected JSON for flags: {e}"))?;
            let serde_json::Value::Array(arr) = json else {
                bail!("expected JSON array for flags");
            };
            let names = arr
                .iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => {
                        let is_valid = flags.flags.iter().any(|f| f.name == *s);
                        if !is_valid {
                            bail!("invalid flag '{s}'");
                        }
                        Ok(s.clone())
                    }
                    _ => bail!("expected string for flag name"),
                })
                .collect::<eyre::Result<Vec<_>>>()?;
            Ok(Val::Flags(names))
        }
        _ => {
            bail!("unsupported type: {:#?}", type_def.kind);
        }
    }
}

/// Strips surrounding double quotes from a string if present.
fn strip_quotes(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

fn print_val(val: Val) {
    let Some(json) = val.try_into_json_value() else {
        return;
    };
    match json {
        serde_json::Value::String(s) => println!("{s}"),
        other => println!("{other}"),
    }
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
