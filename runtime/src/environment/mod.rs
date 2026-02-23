use crate::component::Component;
use crate::component::wit::ComponentInterface;
use crate::resource::ResourceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod deps;

/// Environment manifest - the deployable unit in Asterai.
///
/// An environment bundles one or more components with configuration
/// (environment variables/secrets), forming a versioned, immutable artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    /// Metadata about this environment (namespace, name, version).
    pub metadata: EnvironmentMetadata,
    /// Components in this environment.
    /// Cargo.toml-style: key is "namespace:name", value is "version".
    pub components: HashMap<String, String>,
    /// Environment variables/secrets.
    pub vars: HashMap<String, String>,
}

/// Metadata for an environment manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EnvironmentMetadata {
    pub namespace: String,
    pub name: String,
    pub version: String,
}

/// Reason for a version change when pushing an environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeReason {
    /// First version of the environment.
    Initial,
    /// A component was added.
    ComponentAdded,
    /// A component was removed.
    ComponentRemoved,
    /// A component was upgraded to a new version.
    ComponentUpgraded,
    /// Environment variables were changed.
    VarsChanged,
    /// No changes from the previous version.
    NoChange,
}

impl ChangeReason {
    /// Returns the string representation used in API responses and database.
    pub fn as_str(&self) -> &'static str {
        match self {
            ChangeReason::Initial => "initial",
            ChangeReason::ComponentAdded => "componentAdded",
            ChangeReason::ComponentRemoved => "componentRemoved",
            ChangeReason::ComponentUpgraded => "componentUpgraded",
            ChangeReason::VarsChanged => "varsChanged",
            ChangeReason::NoChange => "noChange",
        }
    }
}

impl std::fmt::Display for ChangeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Environment {
    /// Create a new empty environment.
    pub fn new(namespace: String, name: String, version: String) -> Self {
        Self {
            metadata: EnvironmentMetadata {
                namespace,
                name,
                version,
            },
            components: HashMap::new(),
            vars: HashMap::new(),
        }
    }

    /// Get the namespace of this environment.
    pub fn namespace(&self) -> &str {
        &self.metadata.namespace
    }

    /// Get the name of this environment.
    pub fn name(&self) -> &str {
        &self.metadata.name
    }

    /// Get the version of this environment.
    pub fn version(&self) -> &str {
        &self.metadata.version
    }

    /// Returns true if this is a local unpushed environment.
    /// Local environments use version "0.0.0" as a placeholder.
    pub fn is_local(&self) -> bool {
        self.metadata.version == "0.0.0"
    }

    /// Add a component to this environment.
    pub fn add_component(&mut self, component: &Component) {
        let key = format!("{}:{}", component.namespace(), component.name());
        self.components.insert(key, component.version().to_string());
    }

    /// Remove a component from this environment.
    pub fn remove_component(&mut self, namespace: &str, name: &str) -> bool {
        let key = format!("{}:{}", namespace, name);
        self.components.remove(&key).is_some()
    }

    /// Set an environment variable.
    pub fn set_var(&mut self, key: String, value: String) {
        self.vars.insert(key, value);
    }

    /// Get an environment variable.
    pub fn get_var(&self, key: &str) -> Option<&String> {
        self.vars.get(key)
    }

    /// Get the full resource reference (namespace:name@version).
    pub fn resource_ref(&self) -> String {
        format!(
            "{}:{}@{}",
            self.metadata.namespace, self.metadata.name, self.metadata.version
        )
    }

    /// Get the resource ID (namespace:name) without version.
    pub fn resource_id(&self) -> String {
        format!("{}:{}", self.metadata.namespace, self.metadata.name)
    }

    /// Get display-friendly reference. Shows version only for pushed environments.
    pub fn display_ref(&self) -> String {
        match self.is_local() {
            true => self.resource_id(),
            false => self.resource_ref(),
        }
    }

    /// Get component references as full strings (namespace:name@version).
    pub fn component_refs(&self) -> Vec<String> {
        self.components
            .iter()
            .map(|(id, version)| format!("{}@{}", id, version))
            .collect()
    }

    /// Returns package IDs (e.g. "asterai:fs") of components that are
    /// imported by the loaded components but not exported by any component
    /// in the set. These are dependencies that must be resolved before
    /// the environment can run.
    pub fn dependencies(&self, components: &[impl ComponentInterface]) -> Vec<ResourceId> {
        deps::unsatisfied_import_packages(components)
    }
}
