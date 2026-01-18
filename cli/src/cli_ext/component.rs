use crate::cli_ext::resource_metadata::ResourceMetadataCliExt;
use crate::config::BIN_DIR;
use asterai_runtime::component::Component;
use asterai_runtime::resource::metadata::{ResourceKind, ResourceMetadata};

pub trait ComponentCliExt {
    fn check_does_exist_locally(&self) -> eyre::Result<bool>;
}

impl ComponentCliExt for Component {
    fn check_does_exist_locally(&self) -> eyre::Result<bool> {
        let component_dir = BIN_DIR
            .join("resources")
            .join(self.namespace())
            .join(format!("{}@{}", self.name(), self.version()));
        if !component_dir.exists() {
            return Ok(false);
        }
        let Ok(metadata) = ResourceMetadata::parse_local(&component_dir) else {
            return Ok(false);
        };
        if metadata.kind != ResourceKind::Component {
            return Ok(false);
        }
        Ok(component_dir.join("component.wasm").exists())
    }
}
