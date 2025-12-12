use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct PluginFunctionName {
    pub interface: Option<String>,
    pub name: String,
}

impl PluginFunctionName {
    pub fn new(interface: Option<String>, name: String) -> Self {
        Self { interface, name }
    }
}

impl Display for PluginFunctionName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let interface_string = self
            .interface
            .as_ref()
            .map(|i| format!("{i}/"))
            .unwrap_or_else(|| String::new());
        write!(f, "{interface_string}{}", self.name)
    }
}

impl FromStr for PluginFunctionName {
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

impl Serialize for PluginFunctionName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for PluginFunctionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PluginFunctionName::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid plugin function name: {s}")))
    }
}
