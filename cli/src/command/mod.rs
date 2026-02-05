use crate::command::auth::AuthArgs;
use crate::command::component::ComponentArgs;
use crate::command::env::EnvArgs;
use crate::command::help::Help;
use crate::command::version::Version;

mod auth;
mod common_flags;
mod component;
mod env;
mod help;
mod resource_or_id;
mod version;

#[allow(clippy::large_enum_variant)]
pub enum Command {
    Auth(AuthArgs),
    Env(EnvArgs),
    Component(ComponentArgs),
    Help,
    Version,
}

impl Command {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let Some(first_token) = args.next() else {
            return Ok(Self::Help);
        };
        match first_token.as_str() {
            "auth" => AuthArgs::parse(args).map(Self::Auth),
            "env" => EnvArgs::parse(args).map(Self::Env),
            "component" => ComponentArgs::parse(args).map(Self::Component),
            "-v" | "-V" | "--version" => Ok(Self::Version),
            _ => Ok(Self::Help),
        }
    }

    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Command::Auth(args) => args.execute().await,
            Command::Env(args) => args.execute().await,
            Command::Component(args) => args.execute().await,
            Command::Version => Version::execute(),
            Command::Help => Help::execute(),
        }
    }
}
