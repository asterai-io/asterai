use serde::Deserialize;
use strum_macros::{Display, EnumString};

#[derive(Debug, Copy, Clone, Deserialize)]
pub struct ResourceMetadata {
    pub kind: ResourceKind,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Deserialize, EnumString, Display)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Component,
    Environment,
}
