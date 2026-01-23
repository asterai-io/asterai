use crate::auth::Auth;
use crate::config::{API_URL, API_URL_STAGING};
use eyre::{Context, OptionExt, bail};

#[derive(Debug)]
pub struct DeleteArgs {
    /// Environment reference (namespace:name).
    env_ref: String,
    /// API endpoint.
    endpoint: String,
    /// Use staging environment.
    staging: bool,
    /// Skip confirmation prompt.
    force: bool,
}

impl DeleteArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut env_ref: Option<String> = None;
        let mut endpoint = API_URL.to_string();
        let mut staging = false;
        let mut force = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--endpoint" | "-e" => {
                    endpoint = args.next().ok_or_eyre("missing value for endpoint flag")?;
                }
                "--staging" | "-s" => {
                    staging = true;
                }
                "--force" | "-f" | "-y" | "--yes" => {
                    force = true;
                }
                "--help" | "-h" | "help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if env_ref.is_some() {
                        bail!("unexpected argument: {}", other);
                    }
                    env_ref = Some(other.to_string());
                }
            }
        }
        let env_ref = env_ref.ok_or_eyre(
            "missing environment reference\n\nUsage: asterai env delete <namespace:name>\n\
             Example: asterai env delete myteam:my-env",
        )?;
        Ok(Self {
            env_ref,
            endpoint,
            staging,
            force,
        })
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        let api_key = Auth::read_stored_api_key()
            .ok_or_eyre("API key not found. Run 'asterai auth login' to authenticate.")?;
        // Parse environment reference.
        let (namespace, name) = parse_env_reference(&self.env_ref)?;
        // Confirm deletion unless --force.
        if !self.force {
            println!(
                "WARNING: This will delete environment '{}:{}' and all its versions.",
                namespace, name
            );
            println!("This action cannot be undone.");
            print!("Type the environment name to confirm: ");
            std::io::Write::flush(&mut std::io::stdout())?;

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim();

            if input != name {
                bail!("confirmation failed: expected '{}', got '{}'", name, input);
            }
        }
        println!("deleting environment {}:{}...", namespace, name);
        let base_url = if self.staging {
            API_URL_STAGING
        } else {
            &self.endpoint
        };
        let client = reqwest::Client::new();
        let response = client
            .delete(format!(
                "{}/v1/environment/{}/{}",
                base_url, namespace, name
            ))
            .header("Authorization", api_key.trim())
            .send()
            .await
            .wrap_err("failed to send delete request")?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            bail!("environment '{}:{}' not found", namespace, name);
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            bail!(
                "forbidden: you don't have permission to delete environment '{}:{}'",
                namespace,
                name
            );
        }
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("delete failed ({}): {}", status, error_text);
        }
        println!("deleted environment {}:{}", namespace, name);
        Ok(())
    }
}

/// Parse an environment reference like "namespace:name".
fn parse_env_reference(s: &str) -> eyre::Result<(String, String)> {
    let (namespace, name) = s
        .split_once(':')
        .or_else(|| s.split_once('/'))
        .ok_or_else(|| eyre::eyre!("invalid environment reference '{}': use namespace:name", s))?;

    Ok((namespace.to_string(), name.to_string()))
}

fn print_help() {
    println!(
        r#"Delete an environment from the registry.

Usage: asterai env delete <namespace:name> [options]

Arguments:
  <namespace:name>      Environment to delete

Options:
  -f, --force, -y       Skip confirmation prompt
  -h, --help            Show this help message

Examples:
  asterai env delete myteam:my-env
  asterai env delete myteam:my-env --force
  asterai env delete myteam:my-env --staging
"#
    );
}
