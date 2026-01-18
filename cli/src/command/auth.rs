use crate::auth::Auth;
use eyre::bail;

pub struct AuthArgs {
    action: AuthAction,
}

pub enum AuthAction {
    Login(String),
    Logout,
    Status,
}

impl AuthArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let subcommand = args
            .next()
            .ok_or_else(|| eyre::eyre!("missing subcommand. Expected: login, logout, or status"))?;
        match subcommand.as_str() {
            "login" => {
                let api_key = args
                    .next()
                    .ok_or_else(|| eyre::eyre!("missing <api-key> argument"))?;
                Ok(Self {
                    action: AuthAction::Login(api_key),
                })
            }
            "logout" => Ok(Self {
                action: AuthAction::Logout,
            }),
            "status" => Ok(Self {
                action: AuthAction::Status,
            }),
            _ => bail!("invalid subcommand. Expected: login, logout, or status"),
        }
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        match &self.action {
            AuthAction::Login(api_key) => {
                Auth::store_api_key(api_key)?;
            }
            AuthAction::Logout => {
                Auth::clear_api_key()?;
            }
            AuthAction::Status => {
                unimplemented!()
            }
        }
        Ok(())
    }
}
