use crate::language::Language;
use eyre::{Context, bail};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};
use std::process::Command;

static TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/init/rust");

/// Rust language support.
pub struct Rust;

impl Language for Rust {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn template(&self) -> &'static Dir<'static> {
        &TEMPLATE
    }

    fn is_dir_a_component(&self, dir: &Path) -> bool {
        let cargo_toml = dir.join("Cargo.toml");
        if !cargo_toml.exists() {
            return false;
        }
        // Check if it's a WASM component by looking for the component metadata.
        let Ok(content) = std::fs::read_to_string(&cargo_toml) else {
            return false;
        };
        content.contains("[package.metadata.component]")
    }

    fn build_component(&self, dir: &Path) -> eyre::Result<PathBuf> {
        let status = Command::new("cargo")
            .args(["component", "build", "--release"])
            .current_dir(dir)
            .status()
            .wrap_err("failed to run cargo component build")?;
        if !status.success() {
            bail!("cargo component build failed");
        }
        let crate_name = get_crate_name(dir)?;
        // Underscores in crate names are converted to hyphens in the output.
        let wasm_name = crate_name.replace('-', "_");
        let wasm_path = dir
            .join("target")
            .join("wasm32-wasip2")
            .join("release")
            .join(format!("{}.wasm", wasm_name));
        if !wasm_path.exists() {
            bail!("built WASM file not found at {:?}", wasm_path);
        }
        Ok(wasm_path)
    }
}

/// Extracts the crate name from Cargo.toml.
fn get_crate_name(dir: &Path) -> eyre::Result<String> {
    let cargo_toml = dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .wrap_err_with(|| format!("failed to read {:?}", cargo_toml))?;
    // Simple TOML parsing for the name field.
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("name") {
            if let Some(value) = line.split('=').nth(1) {
                let name = value.trim().trim_matches('"');
                return Ok(name.to_string());
            }
        }
    }
    bail!("could not find crate name in Cargo.toml")
}
