use crate::cli_ext::component::ComponentCliExt;
use crate::command::component::ComponentArgs;
use asterai_runtime::plugin::interface::PluginInterface;
use eyre::OptionExt;

pub(super) struct PkgArgs {
    wit_input_path: String,
    endpoint: Option<String>,
    output: String,
    wit: Option<String>,
}

impl PkgArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        todo!()
    }
}

impl ComponentArgs {
    pub fn pkg(&self) -> eyre::Result<()> {
        let args = self.pkg_args.as_ref().ok_or_eyre("no pkg args")?;
        // TODO
        Ok(())
    }
}
