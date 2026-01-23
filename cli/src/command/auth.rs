use crate::auth::Auth;
use crate::config::{API_URL, API_URL_STAGING};
use eyre::{Context, OptionExt, bail};
use serde::Deserialize;

#[derive(Deserialize)]
struct UserResponse {
    slug: String,
}

pub struct AuthArgs {
    action: AuthAction,
}

pub enum AuthAction {
    Login(String),
    Logout,
    Status { api_endpoint: String },
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
                if api_key.trim().len() < 3 {
                    bail!("invalid api key (too short)");
                }
                Ok(Self {
                    action: AuthAction::Login(api_key),
                })
            }
            "logout" => Ok(Self {
                action: AuthAction::Logout,
            }),
            "status" => {
                let mut api_endpoint = API_URL.to_string();
                let mut staging = false;
                while let Some(arg) = args.next() {
                    match arg.as_str() {
                        "--endpoint" | "-e" => {
                            api_endpoint =
                                args.next().ok_or_eyre("missing value for endpoint flag")?;
                        }
                        "--staging" | "-s" => {
                            staging = true;
                        }
                        other => {
                            if other.starts_with('-') {
                                bail!("unknown flag: {}", other);
                            }
                            bail!("unexpected argument: {}", other);
                        }
                    }
                }
                if staging {
                    api_endpoint = API_URL_STAGING.to_string();
                }
                Ok(Self {
                    action: AuthAction::Status { api_endpoint },
                })
            }
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
            AuthAction::Status { api_endpoint } => {
                let Some(api_key) = Auth::read_stored_api_key() else {
                    println!("you are logged out");
                    return Ok(());
                };
                let slug = validate_api_key(&api_key, api_endpoint).await?;
                println!("logged in as {}", slug);
            }
        }
        Ok(())
    }
}

async fn validate_api_key(api_key: &str, api_endpoint: &str) -> eyre::Result<String> {
    let url = format!("{}/v1/user", api_endpoint);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .send()
        .await
        .wrap_err("failed to connect to API")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!(
            "API key is invalid or expired. \
             Run 'asterai auth login' to re-authenticate."
        );
    }
    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        bail!("failed to validate API key: {}", error_text);
    }
    let user: UserResponse = response
        .json()
        .await
        .wrap_err("failed to parse user response")?;
    Ok(user.slug)
}
