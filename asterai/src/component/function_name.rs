use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// The name of a function, which may have an interface or be part of the root
/// world of the component's package.
/// Note that this function may be part of the component's own package,
/// or implement a function defined in an external package.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ComponentFunctionName {
    pub interface: Option<String>,
    pub name: String,
}

impl ComponentFunctionName {
    pub fn new(interface: Option<String>, name: String) -> Self {
        Self { interface, name }
    }
}

impl Display for ComponentFunctionName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let interface_string = self
            .interface
            .as_ref()
            .map(|i| format!("{i}/"))
            .unwrap_or_else(|| String::new());
        write!(f, "{interface_string}{}", self.name)
    }
}

impl FromStr for ComponentFunctionName {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = match s.split_once('/') {
            None => Self {
                interface: None,
                name: s.to_owned(),
            },
            Some((interface, name)) => Self {
                interface: Some(interface.to_string()),
                name: name.to_string(),
            },
        };
        Ok(value)
    }
}

impl Serialize for ComponentFunctionName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for ComponentFunctionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ComponentFunctionName::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid component function name: {s}")))
    }
}
