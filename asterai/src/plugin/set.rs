use crate::error::AsteraiError;
use crate::plugin::Plugin;
use std::collections::HashSet;
use std::str::FromStr;

pub struct PluginSet(HashSet<Plugin>);

impl PluginSet {
    pub fn inner(&self) -> &HashSet<Plugin> {
        &self.0
    }

    pub fn take(self) -> HashSet<Plugin> {
        self.0
    }
}

impl FromStr for PluginSet {
    type Err = AsteraiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let set = s
            .split(",")
            .map(Plugin::from_str)
            .collect::<Result<HashSet<Plugin>, Self::Err>>()?;
        Ok(Self(set))
    }
}
