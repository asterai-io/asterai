use std::str::Split;

mod init;

pub struct EnvArgs {
    action: &'static str,
    env_name: Option<&'static str>,
    plugin_name: Option<&'static str>,
    env_var: Option<&'static str>,
    instance_id: Option<&'static str>,
}

impl EnvArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        todo!()
    }

    pub async fn run(&self) -> eyre::Result<()> {
        todo!()
    }
}
