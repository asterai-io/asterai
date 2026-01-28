use crate::language::Language;
use eyre::{Context, bail};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};
use std::process::Command;

static TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/init/typescript");

/// TypeScript language support.
pub struct TypeScript;

impl Language for TypeScript {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn template(&self) -> &'static Dir<'static> {
        &TEMPLATE
    }

    fn is_dir_a_component(&self, dir: &Path) -> bool {
        let package_json = dir.join("package.json");
        if !package_json.exists() {
            return false;
        }
        // Check if it's an asterai component by looking for jco in devDependencies.
        let Ok(content) = std::fs::read_to_string(&package_json) else {
            return false;
        };
        content.contains("@bytecodealliance/jco")
    }

    fn get_package_wasm_path(&self, dir: &Path) -> PathBuf {
        dir.join("build").join("package.wasm")
    }

    fn get_component_wasm_path(&self, dir: &Path) -> eyre::Result<PathBuf> {
        Ok(dir.join("build").join("component.wasm"))
    }

    fn build_component(&self, dir: &Path) -> eyre::Result<PathBuf> {
        // Install dependencies if node_modules doesn't exist.
        let node_modules = dir.join("node_modules");
        if !node_modules.exists() {
            let status = Command::new("npm")
                .args(["install"])
                .current_dir(dir)
                .status()
                .wrap_err("failed to run npm install")?;
            if !status.success() {
                bail!("npm install failed");
            }
        }
        // Run npm build.
        let status = Command::new("npm")
            .args(["run", "build"])
            .current_dir(dir)
            .status()
            .wrap_err("failed to run npm run build")?;
        if !status.success() {
            bail!("npm run build failed");
        }
        let wasm_path = self.get_component_wasm_path(dir)?;
        if !wasm_path.exists() {
            bail!("built WASM file not found at {:?}", wasm_path);
        }
        Ok(wasm_path)
    }
}
