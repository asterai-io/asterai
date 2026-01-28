use crate::command::component::ComponentArgs;
use crate::command::component::pkg::run_pkg;
use crate::language;
use eyre::{Context, OptionExt, bail};

#[derive(Debug)]
pub(super) struct BuildArgs;

impl BuildArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        if let Some(arg) = args.next() {
            bail!("unexpected argument: {}", arg);
        }
        Ok(Self)
    }
}

impl ComponentArgs {
    pub async fn build(&self) -> eyre::Result<()> {
        let cwd = std::env::current_dir().wrap_err("failed to get current directory")?;
        let lang = language::detect(&cwd)
            .ok_or_eyre("could not detect component language in current directory")?;
        println!("Detected {} component", lang.name());
        // Generate package.wasm and package.wit from the WIT file.
        let wit_file = lang.get_wit_file_path(&cwd);
        let pkg_wasm = lang.get_package_wasm_path(&cwd);
        let pkg_wit = lang.get_package_wit_path(&cwd);
        run_pkg(&wit_file, &pkg_wasm, Some(&pkg_wit), &self.api_endpoint).await?;
        // Run the language-specific build.
        let wasm_path = lang.build_component(&cwd)?;
        println!("Component built at {:?}", wasm_path);
        Ok(())
    }
}
