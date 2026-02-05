use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct PluginLog {
    pub timestamp_unix: u64,
    pub category: PluginLogCategory,
    pub content: String,
}

#[derive(Copy, Clone, Eq, PartialEq, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum PluginLogCategory {
    Trace,
    Debug,
    Error,
    Warn,
    Info,
}

impl PluginLogCategory {
    pub fn to_db_string(&self) -> String {
        match self {
            PluginLogCategory::Trace => "trc",
            PluginLogCategory::Debug => "dbg",
            PluginLogCategory::Error => "err",
            PluginLogCategory::Warn => "war",
            PluginLogCategory::Info => "inf",
        }
        .to_owned()
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "trc" => Some(Self::Trace),
            "dbg" => Some(Self::Debug),
            "err" => Some(Self::Error),
            "war" => Some(Self::Warn),
            "inf" => Some(Self::Info),
            _ => None,
        }
    }
}

impl Display for PluginLogCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            PluginLogCategory::Trace => "trace",
            PluginLogCategory::Debug => "debug",
            PluginLogCategory::Error => "error",
            PluginLogCategory::Warn => "warn",
            PluginLogCategory::Info => "info",
        };
        write!(f, "{str}")
    }
}
