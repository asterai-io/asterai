use std::str::Split;

pub struct LoginArgs {
    api_key: &'static str,
}

impl LoginArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        todo!()
    }

    pub async fn run(&self) -> eyre::Result<()> {
        todo!()
    }
}
