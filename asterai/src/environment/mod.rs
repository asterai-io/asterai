use crate::plugin::Plugin;
use crate::resource::Resource;
use derive_getters::Getters;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Getters, Serialize, Deserialize)]
pub struct Environment {
    resource: Resource,
    plugins: HashSet<Plugin>,
    vars: HashMap<String, String>,
}

impl Environment {
    pub fn new(resource: Resource) -> Self {
        Self {
            resource,
            plugins: Default::default(),
            vars: Default::default(),
        }
    }
}
