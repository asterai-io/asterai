use asterai_runtime::resource::{Resource, ResourceId};
use eyre::eyre;
use std::path::Path;

pub mod environment;
pub mod resource;
pub mod resource_metadata;

fn resource_from_path(path: &Path) -> eyre::Result<Resource> {
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
    resource_id.with_version(&version)
}
