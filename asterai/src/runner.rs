use crate::component::set::PluginSet;
use crate::error::AsteraiResult;
use std::collections::HashMap;

pub struct Runner {}

pub struct RunArgs {
    env_vars: HashMap<String, String>,
    components: PluginSet,
}

impl Runner {
    pub async fn run(args: RunArgs) -> AsteraiResult<()> {
        Ok(())
    }
}
