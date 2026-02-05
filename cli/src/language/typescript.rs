use crate::language::Language;
use eyre::{Context, bail};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs a command in the given directory, failing if it exits non-zero.
fn run_command(dir: &Path, program: &str, args: &[&str]) -> eyre::Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .wrap_err_with(|| format!("failed to run {}", program))?;
    if !status.success() {
        bail!("{} {} failed", program, args.join(" "));
    }
    Ok(())
}

static TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/init/typescript");

/// TypeScript language support.
pub struct TypeScript;

impl Language for TypeScript {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["ts"]
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

    fn get_wit_file_path(&self, dir: &Path) -> PathBuf {
        dir.join("component.wit")
    }

    fn get_package_wasm_path(&self, dir: &Path) -> PathBuf {
        dir.join("build").join("package.wasm")
    }

    fn get_package_wit_path(&self, dir: &Path) -> PathBuf {
        dir.join("build").join("package.wit")
    }

    fn get_component_wasm_path(&self, dir: &Path) -> eyre::Result<PathBuf> {
        Ok(dir.join("build").join("component.wasm"))
    }

    fn build_component(&self, dir: &Path) -> eyre::Result<PathBuf> {
        if !dir.join("node_modules").exists() {
            run_command(dir, "npm", &["install"])?;
        }
        run_command(dir, "npm", &["run", "build"])?;
        let wasm_path = self.get_component_wasm_path(dir)?;
        if !wasm_path.exists() {
            bail!("built WASM file not found at {:?}", wasm_path);
        }
        Ok(wasm_path)
    }
}
