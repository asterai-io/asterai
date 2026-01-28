use eyre::Result;
use include_dir::Dir;
use std::path::{Path, PathBuf};

mod rust;
mod typescript;

pub use rust::Rust;
pub use typescript::TypeScript;

/// Trait for language-specific component operations.
pub trait Language {
    /// Returns the language name.
    fn name(&self) -> &'static str;

    /// Returns the embedded template directory for this language.
    fn template(&self) -> &'static Dir<'static>;

    /// Checks if the given directory contains a component of this language.
    fn is_dir_a_component(&self, dir: &Path) -> bool;

    /// Returns the expected path to the built package.wasm (WIT interface).
    fn get_package_wasm_path(&self, dir: &Path) -> PathBuf;

    /// Returns the expected path to the built component.wasm (implementation).
    fn get_component_wasm_path(&self, dir: &Path) -> Result<PathBuf>;

    /// Builds the component in the given directory.
    /// Returns the path to the built WASM file.
    fn build_component(&self, dir: &Path) -> Result<PathBuf>;
}

/// Returns all supported languages.
pub fn all() -> Vec<Box<dyn Language>> {
    vec![Box::new(Rust), Box::new(TypeScript)]
}

/// Detects the language of a component in the given directory.
/// Returns `None` if no supported language is detected.
pub fn detect(dir: &Path) -> Option<Box<dyn Language>> {
    for lang in all() {
        if lang.is_dir_a_component(dir) {
            return Some(lang);
        }
    }
    None
}
