use crate::auth::{clear_api_key, store_api_key};
use eyre::bail;

pub struct AuthArgs {
    action: AuthAction,
}

enum AuthAction {
    Clear,
    Set(String),
}

impl AuthArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let content = args.collect::<String>().trim().to_owned();
        if content.is_empty() {
            bail!("missing argument. To clear your API key, use `auth clear`");
        }
        let action = match content == "clear" {
            true => AuthAction::Clear,
            false => AuthAction::Set(content),
        };
        Ok(Self { action })
    }

    pub async fn run(&self) -> eyre::Result<()> {
        match &self.action {
            AuthAction::Clear => {
                clear_api_key()?;
            }
            AuthAction::Set(api_key) => {
                store_api_key(api_key)?;
            }
        }
        Ok(())
    }
}
