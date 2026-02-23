use crate::component::binary::ComponentBinary;
use crate::component::wit::ComponentInterface;
use crate::resource::ResourceId;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

/// Returns package IDs (e.g. "asterai:fs") whose interfaces are imported by
/// at least one component but not exported by any component in the set.
/// These represent dependencies that must be auto-resolved.
pub fn unsatisfied_import_packages(components: &[impl ComponentInterface]) -> Vec<ResourceId> {
    let mut provided: HashSet<ResourceId> = HashSet::new();
    for comp in components {
        for export in comp.exported_interfaces() {
            if let Some(id) = extract_package_id(&export.name) {
                provided.insert(id);
            }
        }
    }

    let mut missing: Vec<ResourceId> = Vec::new();
    let mut seen: HashSet<ResourceId> = HashSet::new();
    for comp in components {
        for import in comp.imported_interfaces() {
            let Some(id) = extract_package_id(&import.name) else {
                continue;
            };
            if is_host_provided_id(&id) {
                continue;
            }
            if provided.contains(&id) {
                continue;
            }
            if seen.insert(id.clone()) {
                missing.push(id);
            }
        }
    }

    missing
}

/// Returns interfaces that are both imported by some component AND exported
/// by more than one component. Only these are problematic — the linker must
/// pick one provider. Duplicate exports that nothing imports are harmless.
pub fn conflicting_exports(components: &[ComponentBinary]) -> Vec<(String, Vec<String>)> {
    // Collect all imported interface names (excluding host-provided).
    let mut imported: HashSet<String> = HashSet::new();
    for comp in components {
        for import in comp.imported_interfaces() {
            if let Some(id) = extract_package_id(&import.name) {
                if !is_host_provided_id(&id) {
                    imported.insert(import.name.clone());
                }
            }
        }
    }

    // Find exports that appear more than once AND are actually imported.
    let mut export_map: HashMap<String, Vec<String>> = HashMap::new();
    for comp in components {
        let comp_id = format!(
            "{}:{}",
            comp.component().namespace(),
            comp.component().name()
        );
        for export in comp.exported_interfaces() {
            if imported.contains(&export.name) {
                export_map
                    .entry(export.name.clone())
                    .or_default()
                    .push(comp_id.clone());
            }
        }
    }
    export_map
        .into_iter()
        .filter(|(_, providers)| providers.len() > 1)
        .map(|(iface, mut providers)| {
            // Sort so the first entry matches the linker's alphabetical
            // instantiation order (the one that will actually be used).
            providers.sort();
            (iface, providers)
        })
        .collect()
}

/// Returns true if the package is provided by the runtime host rather than
/// by a component. These imports should not trigger dependency resolution.
fn is_host_provided_id(id: &ResourceId) -> bool {
    let ns = id.namespace();
    let name = id.name();
    // All WASI interfaces are host-provided.
    if ns == "wasi" {
        return true;
    }
    // asterai host interfaces provided by the runtime.
    if ns == "asterai" && name.starts_with("host") {
        return true;
    }
    false
}

/// Extracts the package identifier ("namespace:package") from a fully
/// qualified interface name like "namespace:package/interface@version".
fn extract_package_id(interface_name: &str) -> Option<ResourceId> {
    let pkg = interface_name.split('/').next()?;
    // Strip any version suffix from the package part itself
    // (e.g. "asterai:fs@1.0.0" → "asterai:fs"), though this is uncommon.
    let pkg = pkg.split('@').next().unwrap_or(pkg);
    ResourceId::from_str(pkg).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_id() {
        let id = extract_package_id("asterai:fs/fs@1.0.0").unwrap();
        assert_eq!(id.namespace(), "asterai");
        assert_eq!(id.name(), "fs");

        let id = extract_package_id("asterai:host/api@1.0.0").unwrap();
        assert_eq!(id.namespace(), "asterai");
        assert_eq!(id.name(), "host");

        let id = extract_package_id("wasi:http/outgoing-handler@0.2.0").unwrap();
        assert_eq!(id.namespace(), "wasi");
        assert_eq!(id.name(), "http");

        assert!(extract_package_id("bare-name").is_none());
    }

    #[test]
    fn test_is_host_provided() {
        let check = |s: &str| -> bool {
            let id = ResourceId::from_str(s).unwrap();
            is_host_provided_id(&id)
        };
        assert!(check("wasi:cli"));
        assert!(check("wasi:http"));
        assert!(check("wasi:filesystem"));
        assert!(check("asterai:host"));
        assert!(check("asterai:host-ws"));
        assert!(check("asterai:host-cron"));
        assert!(check("asterai:host-abc123"));
        assert!(!check("asterai:fs"));
        assert!(!check("asterai:telegram"));
        assert!(!check("asterai:s3"));
        assert!(!check("asterbot:host"));
    }
}
