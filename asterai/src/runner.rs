use crate::component::set::PluginSet;
use crate::error::AsteraiResult;
use std::collections::HashMap;

pub struct Runner {}

#[allow(dead_code)]
pub struct RunArgs {
    env_vars: HashMap<String, String>,
    components: PluginSet,
}

impl Runner {
    pub async fn run(_args: RunArgs) -> AsteraiResult<()> {
        Ok(())
    }
}
