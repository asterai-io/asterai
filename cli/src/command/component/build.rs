use crate::command::component::ComponentArgs;
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
        let wasm_path = lang.build_component(&cwd, &self.api_endpoint).await?;
        println!("Component built at {:?}", wasm_path);
        Ok(())
    }
}
