use crate::cli_ext::resource::ResourceCliExt;
use crate::cli_ext::resource_from_path;
use crate::cli_ext::resource_metadata::ResourceMetadataCliExt;
use asterai_runtime::plugin::Plugin;
use asterai_runtime::plugin::interface::PluginInterface;
use asterai_runtime::resource::metadata::{ResourceKind, ResourceMetadata};
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::bail;
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub trait ComponentCliExt: Sized {
    fn local_list() -> Vec<Self>;
    fn parse_local(path: &Path) -> eyre::Result<Self>;
    /// Fetches the most recent with the given ID.
    fn local_fetch(id: &ResourceId) -> eyre::Result<Self>;
}

impl ComponentCliExt for PluginInterface {
    fn local_list() -> Vec<Self> {
        let resources = Resource::local_list();
        let mut components = Vec::new();
        for resource_path in resources {
            let Ok(metadata) = ResourceMetadata::parse_local(&resource_path) else {
                eprintln!(
                    "ERROR: failed to parse metadata for environment at {}",
                    resource_path.to_str().unwrap_or_default()
                );
                continue;
            };
            if metadata.kind != ResourceKind::Component {
                continue;
            }
            let Ok(env) = Self::parse_local(&resource_path) else {
                eprintln!(
                    "ERROR: failed to parse environment at {}",
                    resource_path.to_str().unwrap_or_default()
                );
                continue;
            };
            components.push(env);
        }
        components
    }

    fn parse_local(path: &Path) -> eyre::Result<Self> {
        let resource = resource_from_path(path)?;
        let component_path = path.to_owned().join("package.wasm");
        let component_bytes = fs::read(&component_path)?;
        let plugin = Plugin::from_str(&resource.to_string())?;
        let item = Self::from_component_bytes(plugin, component_bytes)?;
        Ok(item)
    }

    fn local_fetch(id: &ResourceId) -> eyre::Result<Self> {
        let path = Resource::local_fetch_path(id)?;
        let environment = Self::parse_local(&path)?;
        Ok(environment)
    }
}
