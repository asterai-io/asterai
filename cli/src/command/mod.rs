use crate::command::auth::AuthArgs;
use crate::command::env::EnvArgs;
use eyre::bail;

mod auth;
mod env;

pub enum Command {
    Auth(AuthArgs),
    Env(EnvArgs),
}

impl Command {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(first_token) = args.next() else {
            bail!("no input");
        };
        match first_token.as_str() {
            "auth" => AuthArgs::parse(args).map(Self::Auth),
            "env" => EnvArgs::parse(args).map(Self::Env),
            _ => {
                bail!("invalid command")
            }
        }
    }

    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Command::Auth(args) => args.run().await,
            Command::Env(args) => args.run().await,
        }
    }
}
