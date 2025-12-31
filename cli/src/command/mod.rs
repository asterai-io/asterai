use crate::command::auth::AuthArgs;
use crate::command::env::EnvArgs;
use crate::command::help::Help;
use eyre::bail;

mod auth;
mod env;
mod help;

pub enum Command {
    Auth(AuthArgs),
    Env(EnvArgs),
    Help,
}

impl Command {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(first_token) = args.next() else {
            bail!("no input");
        };
        match first_token.as_str() {
            "auth" => AuthArgs::parse(args).map(Self::Auth),
            "env" => EnvArgs::parse(args).map(Self::Env),
            _ => Ok(Self::Help),
        }
    }

    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Command::Auth(args) => args.execute().await,
            Command::Env(args) => args.execute().await,
            _ => Help::execute(),
        }
    }
}
