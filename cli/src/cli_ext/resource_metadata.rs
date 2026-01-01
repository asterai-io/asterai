use asterai_runtime::resource::metadata::ResourceMetadata;
use std::fs;
use std::path::Path;

pub trait ResourceMetadataCliExt: Sized {
    fn parse_local(path: &Path) -> eyre::Result<Self>;
}

impl ResourceMetadataCliExt for ResourceMetadata {
    fn parse_local(path: &Path) -> eyre::Result<Self> {
        let env_json_path = path.to_owned().join("metadata.json");
        let serialized = fs::read_to_string(&env_json_path)?;
        let metadata: ResourceMetadata = serde_json::from_str(&serialized)?;
        Ok(metadata)
    }
}
