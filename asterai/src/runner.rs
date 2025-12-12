use crate::error::AsteraiResult;
use crate::plugin::set::PluginSet;
use std::collections::HashMap;

pub struct Runner {}

pub struct RunArgs {
    env_vars: HashMap<String, String>,
    plugins: PluginSet,
}

impl Runner {
    pub async fn run(args: RunArgs) -> AsteraiResult<()> {
        Ok(())
    }
}
