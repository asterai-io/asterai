use eyre::bail;
use serde_json::Value;
use wasmtime::component::Val;
use wit_parser::{Type, TypeDef, TypeDefKind};

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

/// Converts a serde_json Value to a wasmtime Val based on the expected WIT type.
pub fn json_value_to_val(value: &Value, ty: &Type) -> eyre::Result<Val> {
    match ty {
        Type::String => match value {
            Value::String(s) => Ok(Val::String(s.clone())),
            _ => bail!("expected string"),
        },
        Type::Bool => match value {
            Value::Bool(b) => Ok(Val::Bool(*b)),
            _ => bail!("expected bool"),
        },
        Type::U8 => Ok(Val::U8(json_to_u64(value)? as u8)),
        Type::U16 => Ok(Val::U16(json_to_u64(value)? as u16)),
        Type::U32 => Ok(Val::U32(json_to_u64(value)? as u32)),
        Type::U64 => Ok(Val::U64(json_to_u64(value)?)),
        Type::S8 => Ok(Val::S8(json_to_i64(value)? as i8)),
        Type::S16 => Ok(Val::S16(json_to_i64(value)? as i16)),
        Type::S32 => Ok(Val::S32(json_to_i64(value)? as i32)),
        Type::S64 => Ok(Val::S64(json_to_i64(value)?)),
        Type::F32 => Ok(Val::Float32(json_to_f64(value)? as f32)),
        Type::F64 => Ok(Val::Float64(json_to_f64(value)?)),
        Type::Char => match value {
            Value::String(s) => {
                let mut chars = s.chars();
                let c = chars.next().ok_or_else(|| eyre::eyre!("expected char"))?;
                if chars.next().is_some() {
                    bail!("expected single char");
                }
                Ok(Val::Char(c))
            }
            _ => bail!("expected string for char"),
        },
        Type::Id(_) => bail!("unresolved type reference in JSON"),
    }
}

/// Parses a string into a wasmtime Val based on the expected WIT primitive type.
pub fn parse_primitive(arg: &str, ty: &Type) -> eyre::Result<Val> {
    match ty {
        Type::String => Ok(Val::String(arg.to_owned())),
        Type::Bool => {
            let v: bool = arg.parse().map_err(|_| eyre::eyre!("expected bool"))?;
            Ok(Val::Bool(v))
        }
        Type::U8 => Ok(Val::U8(arg.parse()?)),
        Type::U16 => Ok(Val::U16(arg.parse()?)),
        Type::U32 => Ok(Val::U32(arg.parse()?)),
        Type::U64 => Ok(Val::U64(arg.parse()?)),
        Type::S8 => Ok(Val::S8(arg.parse()?)),
        Type::S16 => Ok(Val::S16(arg.parse()?)),
        Type::S32 => Ok(Val::S32(arg.parse()?)),
        Type::S64 => Ok(Val::S64(arg.parse()?)),
        Type::F32 => Ok(Val::Float32(arg.parse()?)),
        Type::F64 => Ok(Val::Float64(arg.parse()?)),
        Type::Char => {
            let mut chars = arg.chars();
            let c = chars.next().ok_or_else(|| eyre::eyre!("expected char"))?;
            if chars.next().is_some() {
                bail!("expected single char, got multiple");
            }
            Ok(Val::Char(c))
        }
        Type::Id(_) => bail!("unresolved type reference"),
    }
}

fn json_to_u64(value: &Value) -> eyre::Result<u64> {
    value
        .as_u64()
        .ok_or_else(|| eyre::eyre!("expected unsigned integer"))
}

fn json_to_i64(value: &Value) -> eyre::Result<i64> {
    value
        .as_i64()
        .ok_or_else(|| eyre::eyre!("expected integer"))
}

fn json_to_f64(value: &Value) -> eyre::Result<f64> {
    value.as_f64().ok_or_else(|| eyre::eyre!("expected number"))
}

/// Converts a JSON value to a wasmtime Val based on the expected WIT TypeDef.
pub fn json_value_to_val_typedef(value: &Value, type_def: &TypeDef) -> eyre::Result<Val> {
    match &type_def.kind {
        TypeDefKind::Type(ty) => json_value_to_val(value, ty),
        TypeDefKind::Record(record) => {
            let Value::Object(map) = value else {
                bail!("expected JSON object for record");
            };
            let fields = record
                .fields
                .iter()
                .map(|field| {
                    let v = map
                        .get(&field.name)
                        .ok_or_else(|| eyre::eyre!("missing field '{}'", field.name))?;
                    let val = json_value_to_val(v, &field.ty)?;
                    Ok((field.name.clone(), val))
                })
                .collect::<eyre::Result<Vec<_>>>()?;
            Ok(Val::Record(fields))
        }
        TypeDefKind::List(ty) => {
            let Value::Array(arr) = value else {
                bail!("expected JSON array for list");
            };
            let vals = arr
                .iter()
                .map(|v| json_value_to_val(v, ty))
                .collect::<eyre::Result<Vec<_>>>()?;
            Ok(Val::List(vals))
        }
        TypeDefKind::Tuple(tuple) => {
            let Value::Array(arr) = value else {
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
            let Value::String(s) = value else {
                bail!("expected string for enum");
            };
            if !e.cases.iter().any(|c| c.name == *s) {
                let cases: Vec<&str> = e.cases.iter().map(|c| c.name.as_str()).collect();
                bail!("invalid enum value '{s}', expected one of: {cases:?}");
            }
            Ok(Val::Enum(s.clone()))
        }
        TypeDefKind::Option(ty) => {
            if value.is_null() {
                return Ok(Val::Option(None));
            }
            let inner = json_value_to_val(value, ty)?;
            Ok(Val::Option(Some(Box::new(inner))))
        }
        TypeDefKind::Flags(flags) => {
            let Value::Array(arr) = value else {
                bail!("expected JSON array for flags");
            };
            let names = arr
                .iter()
                .map(|v| match v {
                    Value::String(s) => {
                        if !flags.flags.iter().any(|f| f.name == *s) {
                            bail!("invalid flag '{s}'");
                        }
                        Ok(s.clone())
                    }
                    _ => bail!("expected string for flag name"),
                })
                .collect::<eyre::Result<Vec<_>>>()?;
            Ok(Val::Flags(names))
        }
        _ => bail!("unsupported type: {:#?}", type_def.kind),
    }
}
