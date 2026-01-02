use crate::cli_ext::resource_from_path;
use crate::config::BIN_DIR;
use asterai_runtime::resource::{Resource, ResourceId};
use eyre::bail;
use std::fs;
use std::path::PathBuf;

pub trait ResourceCliExt {
    fn local_list() -> Vec<PathBuf>;
    fn local_fetch_path(id: &ResourceId) -> eyre::Result<PathBuf>;
}

impl ResourceCliExt for Resource {
    fn local_list() -> Vec<PathBuf> {
        let resources_dir = BIN_DIR.join("resources");
        if !resources_dir.exists() {
            return Vec::new();
        }
        let mut paths = Vec::new();
        let Ok(namespaces) = fs::read_dir(&resources_dir) else {
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

    fn local_fetch_path(id: &ResourceId) -> eyre::Result<PathBuf> {
        let local_resources = Resource::local_list();
        let mut selected_resource_opt = None;
        for resource_path in local_resources {
            let resource = resource_from_path(&resource_path)?;
            if resource.id() != *id {
                continue;
            }
            match &selected_resource_opt {
                None => {
                    selected_resource_opt = Some((resource, resource_path));
                }
                Some((r, _)) => {
                    if resource.version() > r.version() {
                        selected_resource_opt = Some((resource, resource_path));
                    }
                }
            }
        }
        let Some((_, selected_resource_path)) = selected_resource_opt else {
            bail!("no resource found");
        };
        Ok(selected_resource_path)
    }
}
