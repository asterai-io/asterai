use crate::auth::Auth;
use crate::local_store::LocalStore;
use eyre::{Context, OptionExt, bail};
use reqwest::StatusCode;
use std::fs;

#[derive(Debug)]
pub struct DeleteArgs {
    /// Environment reference (namespace:name).
    env_ref: String,
    /// Skip confirmation prompt.
    force: bool,
    /// Delete from remote registry instead of local.
    remote: bool,
}

impl DeleteArgs {
    pub fn parse(args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut env_ref: Option<String> = None;
        let mut force = false;
        let mut remote = false;
        for arg in args {
            match arg.as_str() {
                "--force" | "-f" | "-y" | "--yes" => {
                    force = true;
                }
                "--remote" => {
                    remote = true;
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
            force,
            remote,
        })
    }

    pub async fn execute(&self, api_endpoint: &str) -> eyre::Result<()> {
        let (namespace, name) = parse_env_reference(&self.env_ref)?;
        if self.remote {
            self.execute_remote(&namespace, &name, api_endpoint).await
        } else {
            self.execute_local(&namespace, &name)
        }
    }

    fn execute_local(&self, namespace: &str, name: &str) -> eyre::Result<()> {
        let versions_to_delete = LocalStore::find_all_versions(namespace, name);
        if versions_to_delete.is_empty() {
            bail!("environment '{}:{}' not found locally", namespace, name);
        }
        // Confirm deletion unless --force.
        if !self.force {
            println!(
                "WARNING: This will delete {} local version(s) of environment '{}:{}'.",
                versions_to_delete.len(),
                namespace,
                name
            );
            print!("Type the environment name to confirm: ");
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim();
            if input != name {
                bail!("confirmation failed: expected '{}', got '{}'", name, input);
            }
        }
        // Delete all versions.
        for path in &versions_to_delete {
            let version = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            fs::remove_dir_all(path).wrap_err_with(|| format!("failed to delete {}", version))?;
            println!("deleted {}", version);
        }
        println!(
            "deleted {} version(s) of {}:{}",
            versions_to_delete.len(),
            namespace,
            name
        );
        Ok(())
    }

    async fn execute_remote(
        &self,
        namespace: &str,
        name: &str,
        api_endpoint: &str,
    ) -> eyre::Result<()> {
        let api_key = Auth::read_stored_api_key()
            .ok_or_eyre("API key not found. Run 'asterai auth login' to authenticate.")?;
        // Confirm deletion unless --force.
        if !self.force {
            println!(
                "WARNING: This will delete environment '{}:{}' and all its versions \
                 from the registry.",
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
        println!(
            "deleting environment {}:{} from registry...",
            namespace, name
        );
        let base_url = api_endpoint;
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
        if status == StatusCode::NOT_FOUND {
            bail!("environment '{}:{}' not found in registry", namespace, name);
        }
        if status == StatusCode::FORBIDDEN {
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
        println!("deleted environment {}:{} from registry", namespace, name);
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
        r#"Delete an environment.

By default, deletes the environment locally. Use --remote to delete from the registry.

Usage: asterai env delete <namespace:name> [options]

Arguments:
  <namespace:name>      Environment to delete

Options:
  -r, --remote          Delete from registry instead of locally
  -f, --force, -y       Skip confirmation prompt
  -h, --help            Show this help message

Examples:
  asterai env delete myteam:my-env              # Delete locally
  asterai env delete myteam:my-env --force      # Delete locally, skip confirmation
  asterai env delete myteam:my-env --remote     # Delete from registry
"#
    );
}
