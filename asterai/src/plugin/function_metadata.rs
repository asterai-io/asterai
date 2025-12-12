use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginFunctionMetadata {
    pub is_agentic: bool,
}

impl Default for PluginFunctionMetadata {
    fn default() -> Self {
        Self { is_agentic: true }
    }
}
