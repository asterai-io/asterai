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

    /// Returns the path to the source WIT file.
    fn get_wit_file_path(&self, dir: &Path) -> PathBuf;

    /// Returns the expected path to the built package.wasm (WIT interface).
    fn get_package_wasm_path(&self, dir: &Path) -> PathBuf;

    /// Returns the expected path to the generated package.wit.
    fn get_package_wit_path(&self, dir: &Path) -> PathBuf;

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
    all().into_iter().find(|lang| lang.is_dir_a_component(dir))
}

/// Returns a language by name, or `None` if not found.
pub fn from_name(name: &str) -> Option<Box<dyn Language>> {
    all().into_iter().find(|lang| lang.name() == name)
}

/// Returns a comma-separated list of supported language names.
pub fn supported_names() -> String {
    all()
        .iter()
        .map(|lang| lang.name())
        .collect::<Vec<_>>()
        .join(", ")
}
