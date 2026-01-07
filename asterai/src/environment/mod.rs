use crate::plugin::Plugin;
use crate::resource::Resource;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub resource: Resource,
    pub components: HashSet<Plugin>,
    pub vars: HashMap<String, String>,
}

impl Environment {
    pub fn new(resource: Resource) -> Self {
        Self {
            resource,
            components: Default::default(),
            vars: Default::default(),
        }
    }
}
