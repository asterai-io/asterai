use crate::cli_ext::resource::ResourceCliExt;
use asterai_runtime::resource::Resource;
use eyre::{OptionExt, bail};
use std::fs;

#[derive(Debug)]
pub struct DeleteArgs {
    /// Component reference (namespace:name).
    component_ref: String,
    /// Skip confirmation prompt.
    force: bool,
}

impl DeleteArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut component_ref: Option<String> = None;
        let mut force = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--force" | "-f" | "-y" | "--yes" => {
                    force = true;
                }
                "--help" | "-h" | "help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if component_ref.is_some() {
                        bail!("unexpected argument: {}", other);
                    }
                    component_ref = Some(other.to_string());
                }
            }
        }
        let component_ref = component_ref.ok_or_eyre(
            "missing component reference\n\n\
             Usage: asterai component delete <namespace:name>\n\
             Example: asterai component delete myteam:my-component",
        )?;
        Ok(Self {
            component_ref,
            force,
        })
    }

    pub fn execute(&self) -> eyre::Result<()> {
        let (namespace, name) = parse_component_reference(&self.component_ref)?;
        // Find all versions of this component.
        let versions_to_delete: Vec<_> = Resource::local_find_all_versions(&namespace, &name)
            .into_iter()
            .filter(|path| {
                // Verify it's actually a component (has component.wasm or package.wasm).
                path.join("component.wasm").exists() || path.join("package.wasm").exists()
            })
            .collect();
        if versions_to_delete.is_empty() {
            bail!("component '{}:{}' not found locally", namespace, name);
        }
        // Confirm deletion unless --force.
        if !self.force {
            println!(
                "WARNING: This will delete {} local version(s) of component '{}:{}'.",
                versions_to_delete.len(),
                namespace,
                name
            );
            print!("Type the component name to confirm: ");
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim();
            if input != name {
                bail!("confirmation failed: expected '{}', got '{}'", name, input);
            }
        }
        // Delete all versions.
        for path in &versions_to_delete {
            let version = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            fs::remove_dir_all(path)?;
            println!("deleted {}", version);
        }
        println!(
            "deleted {} version(s) of {}:{}",
            versions_to_delete.len(),
            namespace,
            name
        );
        Ok(())
    }
}

/// Parse a component reference like "namespace:name".
fn parse_component_reference(s: &str) -> eyre::Result<(String, String)> {
    let (namespace, name) = s
        .split_once(':')
        .or_else(|| s.split_once('/'))
        .ok_or_else(|| {
            eyre::eyre!("invalid component reference '{}': use namespace:name", s)
        })?;
    Ok((namespace.to_string(), name.to_string()))
}

fn print_help() {
    println!(
        r#"Delete a component locally.

Usage: asterai component delete <namespace:name> [options]

Arguments:
  <namespace:name>      Component to delete

Options:
  -f, --force, -y       Skip confirmation prompt
  -h, --help            Show this help message

Examples:
  asterai component delete myteam:my-component
  asterai component delete myteam:my-component --force
"#
    );
}
