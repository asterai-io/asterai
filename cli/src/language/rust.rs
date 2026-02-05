use super::run_command;
use crate::command::component::pkg::run_pkg;
use crate::language::Language;
use async_trait::async_trait;
use eyre::{Context, bail};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};

static TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/init/rust");

/// Rust language support.
pub struct Rust;

#[async_trait]
impl Language for Rust {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["rs"]
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

    fn get_wit_file_path(&self, dir: &Path) -> PathBuf {
        dir.join("component.wit")
    }

    fn get_package_wasm_path(&self, dir: &Path) -> PathBuf {
        dir.join("wit").join("package.wasm")
    }

    fn get_package_wit_path(&self, dir: &Path) -> PathBuf {
        dir.join("wit").join("package.wit")
    }

    fn get_component_wasm_path(&self, dir: &Path) -> eyre::Result<PathBuf> {
        let crate_name = get_crate_name(dir)?;
        let wasm_name = crate_name.replace('-', "_");
        let filename = format!("{}.wasm", wasm_name);
        // cargo-component may target wasip1 or wasip2 depending on version.
        // Older versions target wasip1 and wrap to a wasip2 component.
        let candidates = ["wasm32-wasip2", "wasm32-wasip1"];
        for target in candidates {
            let path = dir
                .join("target")
                .join(target)
                .join("release")
                .join(&filename);
            if path.exists() {
                return Ok(path);
            }
        }
        // Default to wasm32-wasip2, though this will error later if
        // used as it does not exist according to the checks above.
        Ok(dir
            .join("target")
            .join("wasm32-wasip2")
            .join("release")
            .join(filename))
    }

    async fn build_component(&self, dir: &Path, api_endpoint: &str) -> eyre::Result<PathBuf> {
        // Generate package.wasm and package.wit from the WIT file.
        let wit_file = self.get_wit_file_path(dir);
        let pkg_wasm = self.get_package_wasm_path(dir);
        let pkg_wit = self.get_package_wit_path(dir);
        run_pkg(&wit_file, &pkg_wasm, Some(&pkg_wit), api_endpoint).await?;
        // Build.
        run_command(dir, "cargo", &["component", "build", "--release"])?;
        let wasm_path = self.get_component_wasm_path(dir)?;
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
        if line.starts_with("name")
            && let Some(value) = line.split('=').nth(1)
        {
            let name = value.trim().trim_matches('"');
            return Ok(name.to_string());
        }
    }
    bail!("could not find crate name in Cargo.toml")
}
