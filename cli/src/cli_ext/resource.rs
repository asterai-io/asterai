use crate::config::BIN_DIR;
use asterai_runtime::resource::Resource;
use std::fs;
use std::path::PathBuf;

pub trait ResourceCliExt {
    fn local_list() -> Vec<PathBuf>;
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
}
