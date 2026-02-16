//! Local storage for environments and components.
//!
//! All artifacts are stored in `~/.local/bin/asterai/artifacts/` with the structure:
//! ```
//! artifacts/
//!   namespace/
//!     name@version/
//!       metadata.json    # ResourceKind (Environment or Component)
//!       env.toml         # For environments
//!       component.wasm   # For components
//!       package.wasm     # For components (WIT interface)
//! ```

use crate::config::ARTIFACTS_DIR;
use asterai_runtime::component::Component;
use asterai_runtime::component::binary::ComponentBinary;
use asterai_runtime::environment::Environment;
use asterai_runtime::resource::metadata::{ResourceKind, ResourceMetadata};
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::{Context, bail, eyre};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Local artifact storage.
pub struct LocalStore;

impl LocalStore {
    /// List all resource paths in the local store.
    pub fn list_all_paths() -> Vec<PathBuf> {
        let artifacts_dir = &*ARTIFACTS_DIR;
        if !artifacts_dir.exists() {
            return Vec::new();
        }
        let mut paths = Vec::new();
        let Ok(namespaces) = fs::read_dir(artifacts_dir) else {
            return Vec::new();
        };
        for namespace in namespaces.flatten() {
            let Ok(entries) = fs::read_dir(namespace.path()) else {
                continue;
            };
            paths.extend(entries.flatten().map(|e| e.path()));
        }
        paths
    }

    /// Find all local versions of a resource by namespace and name.
    pub fn find_all_versions(namespace: &str, name: &str) -> Vec<PathBuf> {
        let namespace_dir = ARTIFACTS_DIR.join(namespace);
        if !namespace_dir.exists() {
            return Vec::new();
        }
        let prefix = format!("{}@", name);
        let Ok(entries) = fs::read_dir(&namespace_dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|s| s.starts_with(&prefix))
                    .unwrap_or(false)
            })
            .map(|entry| entry.path())
            .collect()
    }

    /// Find the path to a resource by ID and kind (returns the most recent version).
    pub fn find_path(id: &ResourceId, kind: ResourceKind) -> eyre::Result<PathBuf> {
        let local_resources = Self::list_all_paths();
        let mut selected: Option<(Resource, PathBuf)> = None;
        for resource_path in local_resources {
            let Ok(metadata) = Self::parse_metadata(&resource_path) else {
                continue;
            };
            if metadata.kind != kind {
                continue;
            }
            let Ok(resource) = Self::resource_from_path(&resource_path) else {
                continue;
            };
            if resource.id() != *id {
                continue;
            }
            match &selected {
                None => {
                    selected = Some((resource, resource_path));
                }
                Some((r, _)) => {
                    if resource.version() > r.version() {
                        selected = Some((resource, resource_path));
                    }
                }
            }
        }
        let Some((_, path)) = selected else {
            bail!("resource not found: {}", id);
        };
        Ok(path)
    }

    /// Parse a Resource from a path.
    pub fn resource_from_path(path: &Path) -> eyre::Result<Resource> {
        let namespace = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .ok_or_else(|| eyre!("invalid namespace in path"))?;
        let name_version = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| eyre!("invalid name@version in path"))?;
        let (name, version) = name_version
            .split_once('@')
            .ok_or_else(|| eyre!("missing version separator '@' in path"))?;
        let resource_id = ResourceId::new_from_parts(namespace.to_owned(), name.to_owned())?;
        resource_id.with_version(version)
    }

    /// Parse resource metadata from a path.
    pub fn parse_metadata(path: &Path) -> eyre::Result<ResourceMetadata> {
        let metadata_path = path.join("metadata.json");
        let serialized = fs::read_to_string(&metadata_path)?;
        let metadata: ResourceMetadata = serde_json::from_str(&serialized)?;
        Ok(metadata)
    }

    /// Delete a resource directory.
    pub fn delete(path: &Path) -> eyre::Result<()> {
        fs::remove_dir_all(path)?;
        Ok(())
    }

    /// List all local environments.
    pub fn list_environments() -> Vec<Environment> {
        let mut envs = Vec::new();
        for resource_path in Self::list_all_paths() {
            let Ok(metadata) = Self::parse_metadata(&resource_path) else {
                continue;
            };
            if metadata.kind != ResourceKind::Environment {
                continue;
            }
            match Self::parse_environment(&resource_path) {
                Ok(env) => envs.push(env),
                Err(e) => {
                    eprintln!(
                        "ERROR: failed to parse environment at {}: {e:#}",
                        resource_path.display()
                    );
                }
            }
        }
        envs
    }

    /// Parse an environment from a path.
    pub fn parse_environment(path: &Path) -> eyre::Result<Environment> {
        let env_toml_path = path.join("env.toml");
        let serialized = fs::read_to_string(&env_toml_path)?;
        let environment: Environment = toml::from_str(&serialized)?;
        // Validate that the environment metadata matches the path.
        let dir_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let expected_dir = format!("{}@{}", environment.name(), environment.version());
        if dir_name != expected_dir {
            bail!("env.toml metadata does not match directory name");
        }
        Ok(environment)
    }

    /// Fetch an environment by ID (returns the most recent version).
    pub fn fetch_environment(id: &ResourceId) -> eyre::Result<Environment> {
        let path = Self::find_path(id, ResourceKind::Environment)?;
        Self::parse_environment(&path)
    }

    /// Get the storage directory for an environment.
    pub fn environment_dir(env: &Environment) -> PathBuf {
        ARTIFACTS_DIR
            .join(env.namespace())
            .join(format!("{}@{}", env.name(), env.version()))
    }

    /// Write an environment to local storage.
    pub fn write_environment(env: &Environment) -> eyre::Result<()> {
        let dir = Self::environment_dir(env);
        fs::create_dir_all(&dir)?;
        // Write env.toml.
        let env_path = dir.join("env.toml");
        let env_serialized = toml::to_string_pretty(env)?;
        fs::write(&env_path, env_serialized).wrap_err("failed to write env.toml")?;
        // Write metadata.json.
        let metadata_path = dir.join("metadata.json");
        let metadata = ResourceMetadata {
            kind: ResourceKind::Environment,
        };
        let metadata_serialized = serde_json::to_string(&metadata)?;
        fs::write(&metadata_path, metadata_serialized).wrap_err("failed to write metadata.json")?;
        Ok(())
    }

    /// List all local components.
    pub fn list_components() -> Vec<ComponentBinary> {
        let mut components = Vec::new();
        for resource_path in Self::list_all_paths() {
            let Ok(metadata) = Self::parse_metadata(&resource_path) else {
                continue;
            };
            if metadata.kind != ResourceKind::Component {
                continue;
            }
            match Self::parse_component(&resource_path) {
                Ok(component) => components.push(component),
                Err(e) => {
                    eprintln!(
                        "ERROR: failed to parse component at {}: {e:#}",
                        resource_path.display()
                    );
                }
            }
        }
        components
    }

    /// Parse a component from a path.
    pub fn parse_component(path: &Path) -> eyre::Result<ComponentBinary> {
        let resource = Self::resource_from_path(path)?;
        let component_path = path.join("component.wasm");
        let component_bytes = fs::read(&component_path)?;
        let component = Component::from_str(&resource.to_string())?;
        let mut binary = ComponentBinary::from_component_bytes(component, component_bytes)?;
        let package_path = path.join("package.wasm");
        if package_path.exists() {
            let package_bytes = fs::read(&package_path)?;
            binary.apply_package_docs(&package_bytes)?;
        }
        Ok(binary)
    }

    /// Fetch a component by ID (returns the most recent version).
    pub fn fetch_component(id: &ResourceId) -> eyre::Result<ComponentBinary> {
        let path = Self::find_path(id, ResourceKind::Component)?;
        Self::parse_component(&path)
    }

    /// Check if a component exists locally.
    pub fn component_exists(component: &Component) -> bool {
        let component_dir = ARTIFACTS_DIR.join(component.namespace()).join(format!(
            "{}@{}",
            component.name(),
            component.version()
        ));
        if !component_dir.exists() {
            return false;
        }
        let Ok(metadata) = Self::parse_metadata(&component_dir) else {
            return false;
        };
        if metadata.kind != ResourceKind::Component {
            return false;
        }
        component_dir.join("component.wasm").exists()
    }
}
