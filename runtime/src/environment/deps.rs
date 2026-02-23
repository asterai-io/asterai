use crate::component::binary::ComponentBinary;
use crate::component::wit::ComponentInterface;
use std::collections::{HashMap, HashSet};

/// Returns component package IDs (e.g. "asterai:fs") whose interfaces are
/// imported by at least one component but not exported by any component in
/// the set. These represent dependencies that must be auto-resolved.
pub fn unsatisfied_import_packages(components: &[ComponentBinary]) -> Vec<String> {
    let mut provided: HashSet<String> = HashSet::new();
    for comp in components {
        // The component's own package counts as provided.
        let comp_id = format!(
            "{}:{}",
            comp.component().namespace(),
            comp.component().name()
        );
        provided.insert(comp_id);
        // Every exported interface's package is also provided.
        for export in comp.exported_interfaces() {
            if let Some(pkg) = extract_package_id(&export.name) {
                provided.insert(pkg);
            }
        }
    }

    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for comp in components {
        for import in comp.imported_interfaces() {
            let Some(pkg) = extract_package_id(&import.name) else {
                continue;
            };
            if is_host_provided(&pkg) {
                continue;
            }
            if provided.contains(&pkg) {
                continue;
            }
            if seen.insert(pkg.clone()) {
                missing.push(pkg);
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
            let Some(pkg) = extract_package_id(&import.name) else {
                continue;
            };
            if !is_host_provided(&pkg) {
                imported.insert(import.name.clone());
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
fn is_host_provided(package: &str) -> bool {
    // All WASI interfaces are host-provided.
    if package.starts_with("wasi:") {
        return true;
    }
    // asterai host interfaces provided by the runtime.
    if package.starts_with("asterai:host") {
        return true;
    }
    false
}

/// Extracts the package identifier ("namespace:package") from a fully
/// qualified interface name like "namespace:package/interface@version".
fn extract_package_id(interface_name: &str) -> Option<String> {
    let pkg = interface_name.split('/').next()?;
    // Strip any version suffix from the package part itself
    // (e.g. "asterai:fs@1.0.0" → "asterai:fs"), though this is uncommon.
    let pkg = pkg.split('@').next().unwrap_or(pkg);
    if pkg.contains(':') {
        Some(pkg.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_id() {
        assert_eq!(
            extract_package_id("asterai:fs/fs@1.0.0"),
            Some("asterai:fs".to_string())
        );
        assert_eq!(
            extract_package_id("asterai:host/api@1.0.0"),
            Some("asterai:host".to_string())
        );
        assert_eq!(
            extract_package_id("wasi:http/outgoing-handler@0.2.0"),
            Some("wasi:http".to_string())
        );
        assert_eq!(extract_package_id("bare-name"), None);
    }

    #[test]
    fn test_is_host_provided() {
        assert!(is_host_provided("wasi:cli"));
        assert!(is_host_provided("wasi:http"));
        assert!(is_host_provided("wasi:filesystem"));
        assert!(is_host_provided("asterai:host"));
        assert!(is_host_provided("asterai:host-ws"));
        assert!(is_host_provided("asterai:host-cron"));
        assert!(is_host_provided("asterai:host-abc123"));
        assert!(!is_host_provided("asterai:fs"));
        assert!(!is_host_provided("asterai:telegram"));
        assert!(!is_host_provided("asterai:s3"));
        assert!(!is_host_provided("asterbot:host"));
    }
}
