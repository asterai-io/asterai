use crate::component::interface::ComponentFunctionInterface;
use crate::runtime::SerializableVal;
use derive_getters::Getters;
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};
use wasmtime::component::Val;
use wit_parser::TypeDef;

#[derive(Getters, Clone)]
pub struct ComponentOutput {
    // TODO: rename to structured_output_opt?
    pub function_output_opt: Option<ComponentFunctionOutput>,
    /// The output that the agent sees.
    /// This may be an overrided response, or a serialized version
    /// of `function_output_opt`.
    // TODO: rename to natural_language_output_opt?
    pub component_response_to_agent_opt: Option<String>,
}

#[derive(Clone)]
pub struct ComponentFunctionOutput {
    pub type_def: TypeDef,
    pub value: SerializableVal,
    pub(super) function_interface: ComponentFunctionInterface,
}

impl Serialize for ComponentFunctionOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ComponentOutput", 4)?;
        state.serialize_field("component", &self.function_interface.component().id())?;
        state.serialize_field(
            "version",
            &self.function_interface.component().version().to_string(),
        )?;
        state.serialize_field("function", self.function_interface.name())?;
        state.serialize_field("value", &self.value)?;
        state.end()
    }
}

impl Serialize for SerializableVal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.val {
            Val::Bool(v) => serializer.serialize_bool(*v),
            Val::S8(v) => serializer.serialize_i8(*v),
            Val::U8(v) => serializer.serialize_u8(*v),
            Val::S16(v) => serializer.serialize_i16(*v),
            Val::U16(v) => serializer.serialize_u16(*v),
            Val::S32(v) => serializer.serialize_i32(*v),
            Val::U32(v) => serializer.serialize_u32(*v),
            Val::S64(v) => serializer.serialize_i64(*v),
            Val::U64(v) => serializer.serialize_u64(*v),
            Val::Float32(v) => serializer.serialize_f32(*v),
            Val::Float64(v) => serializer.serialize_f64(*v),
            Val::Char(v) => serializer.serialize_char(*v),
            Val::String(v) => serializer.serialize_str(&v),
            Val::List(v) | Val::Tuple(v) => {
                let mut state = serializer.serialize_seq(Some(v.len()))?;
                for child in v.to_owned() {
                    state.serialize_element(&SerializableVal {
                        name: None,
                        val: child,
                    })?;
                }
                state.end()
            }
            Val::Record(v) => {
                let name = self
                    .name
                    .clone()
                    .unwrap_or_else(|| "ComponentOutput".to_owned());
                // TODO fix leak
                let mut state =
                    serializer.serialize_struct(Box::leak(name.into_boxed_str()), v.len())?;
                for (key, val) in v.to_owned().into_iter() {
                    state.serialize_field(
                        // TODO fix leak
                        Box::leak(key.into_boxed_str()),
                        &SerializableVal {
                            name: None,
                            val: val.clone(),
                        },
                    )?;
                }
                state.end()
            }
            Val::Enum(v) => serializer.serialize_str(&v),
            Val::Option(v_opt) => {
                if let Some(v) = v_opt.clone() {
                    SerializableVal {
                        name: None,
                        val: *v,
                    }
                    .serialize(serializer)
                } else {
                    serializer.serialize_none()
                }
            }
            Val::Result(result) => match result {
                Ok(v_opt) => {
                    if let Some(v) = v_opt.clone() {
                        SerializableVal {
                            name: None,
                            val: *v,
                        }
                        .serialize(serializer)
                    } else {
                        serializer.serialize_none()
                    }
                }
                Err(v_opt) => {
                    if let Some(v) = v_opt.clone() {
                        SerializableVal {
                            name: None,
                            val: *v,
                        }
                        .serialize(serializer)
                    } else {
                        serializer.serialize_none()
                    }
                }
            },
            _ => Err(serde::ser::Error::custom("unsupported Val variant")),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::component::Component;
    use crate::component::function_name::ComponentFunctionName;
    use std::str::FromStr;
    use wit_parser::{TypeDefKind, TypeOwner};

    #[test]
    fn test_serialize_component_function_output_struct() {
        let output = ComponentFunctionOutput {
            type_def: TypeDef {
                name: None,
                kind: TypeDefKind::Resource,
                owner: TypeOwner::None,
                docs: Default::default(),
                stability: Default::default(),
            },
            value: SerializableVal {
                name: None,
                val: Val::Record(vec![("foo".to_owned(), Val::String("bar".to_owned()))]),
            },
            function_interface: ComponentFunctionInterface {
                name: ComponentFunctionName::from_str("important_function").unwrap(),
                inputs: vec![],
                output_type: None,
                component: Component::from_str("namespace:component@0.1.0").unwrap(),
                package_name: wit_parser::PackageName {
                    namespace: "namespace".to_owned(),
                    name: "component".to_owned(),
                    version: Some(semver::Version::new(0, 1, 0)),
                },
            },
        };
        let serialized = serde_json::to_string(&output).unwrap();
        let expected = r#"{"component":"namespace:component","version":"0.1.0","function":"important_function","value":{"foo":"bar"}}"#;
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_component_function_output_number() {
        let output = ComponentFunctionOutput {
            type_def: TypeDef {
                name: None,
                kind: TypeDefKind::Resource,
                owner: TypeOwner::None,
                docs: Default::default(),
                stability: Default::default(),
            },
            value: SerializableVal {
                name: None,
                val: Val::U32(1337),
            },
            function_interface: ComponentFunctionInterface {
                name: ComponentFunctionName::from_str("important_function").unwrap(),
                inputs: vec![],
                output_type: None,
                component: Component::from_str("namespace:component@0.1.0").unwrap(),
                package_name: wit_parser::PackageName {
                    namespace: "namespace".to_owned(),
                    name: "component".to_owned(),
                    version: Some(semver::Version::new(0, 1, 0)),
                },
            },
        };
        let serialized = serde_json::to_string(&output).unwrap();
        let expected = r#"{"component":"namespace:component","version":"0.1.0","function":"important_function","value":1337}"#;
        assert_eq!(serialized, expected);
    }
}
