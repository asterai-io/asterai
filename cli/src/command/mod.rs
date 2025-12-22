use crate::command::env::EnvArgs;
use crate::command::login::LoginArgs;
use eyre::bail;

mod env;
mod login;

pub enum Command {
    Login(LoginArgs),
    Env(EnvArgs),
}

impl Command {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(first_token) = args.next() else {
            bail!("no input");
        };
        match first_token.as_str() {
            "login" => LoginArgs::parse(args).map(Self::Login),
            "env" => EnvArgs::parse(args).map(Self::Env),
            _ => {
                bail!("invalid command")
            }
        }
    }

    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Command::Login(args) => args.run().await,
            Command::Env(args) => args.run().await,
        }
    }
}
