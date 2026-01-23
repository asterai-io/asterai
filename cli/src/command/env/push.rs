use crate::auth::Auth;
use crate::config::{API_URL, API_URL_STAGING};
use crate::local_store::LocalStore;
use asterai_runtime::resource::ResourceId;
use eyre::{Context, OptionExt, bail};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug)]
pub struct PushArgs {
    env_name: String,
    endpoint: String,
    staging: bool,
}

/// Request body for pushing an environment.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PutEnvironmentRequest {
    components: Vec<String>,
    vars: HashMap<String, String>,
}

/// Response from pushing an environment.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutEnvironmentResponse {
    namespace: String,
    name: String,
    version: String,
    previous_version: Option<String>,
    change_type: Option<String>,
    change_reason: String,
}

impl PushArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut env_name: Option<String> = None;
        let mut endpoint = API_URL.to_string();
        let mut staging = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--endpoint" | "-e" => {
                    endpoint = args.next().ok_or_eyre("missing value for endpoint flag")?;
                }
                "--staging" | "-s" => {
                    staging = true;
                }
                "--help" | "-h" | "help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if env_name.is_some() {
                        bail!("unexpected argument: {}", other);
                    }
                    env_name = Some(other.to_string());
                }
            }
        }

        let env_name = env_name.ok_or_eyre(
            "missing environment name\n\nUsage: asterai env push <name>\n\
             Example: asterai env push my-env",
        )?;

        Ok(Self {
            env_name,
            endpoint,
            staging,
        })
    }

    pub async fn execute(&self) -> eyre::Result<()> {
        let api_key = Auth::read_stored_api_key()
            .ok_or_eyre("API key not found. Run 'asterai auth login' to authenticate.")?;

        // Parse environment name and fetch from local storage.
        let resource_id = ResourceId::from_str(&self.env_name)
            .or_else(|_| {
                // Try with fallback namespace.
                let with_namespace = format!(
                    "{}:{}",
                    Auth::read_user_or_fallback_namespace(),
                    self.env_name
                );
                ResourceId::from_str(&with_namespace)
            })
            .wrap_err("invalid environment name")?;

        let environment = LocalStore::fetch_environment(&resource_id)
            .wrap_err_with(|| format!("environment '{}' not found locally", self.env_name))?;

        let namespace = environment.namespace();
        let name = environment.name();

        println!("pushing environment {}:{}...", namespace, name);

        // Convert components to API format (namespace:name@version).
        let components: Vec<String> = environment.component_refs();

        // Build request.
        let request = PutEnvironmentRequest {
            components,
            vars: environment.vars.clone(),
        };

        // Determine base URL.
        let base_url = if self.staging {
            API_URL_STAGING
        } else {
            &self.endpoint
        };

        let client = reqwest::Client::new();
        let response = client
            .put(format!(
                "{}/v1/environment/{}/{}",
                base_url, namespace, name
            ))
            .header("Authorization", api_key.trim())
            .json(&request)
            .send()
            .await
            .wrap_err("failed to send push request")?;

        let status = response.status();

        if status == StatusCode::CONFLICT {
            let body = response.text().await.unwrap_or_default();
            bail!("conflict: {}", body);
        }

        if status == StatusCode::FORBIDDEN {
            bail!(
                "forbidden: you don't have permission to push to namespace '{}'",
                namespace
            );
        }

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("push failed ({}): {}", status, error_text);
        }

        let result: PutEnvironmentResponse =
            response.json().await.wrap_err("failed to parse response")?;

        // Display result.
        if let Some(prev) = &result.previous_version {
            println!(
                "updated {}:{}@{} (was {})",
                result.namespace, result.name, result.version, prev
            );
            if let Some(change_type) = &result.change_type {
                println!("  change: {} ({})", change_type, result.change_reason);
            }
        } else {
            println!(
                "created {}:{}@{}",
                result.namespace, result.name, result.version
            );
        }

        Ok(())
    }
}

fn print_help() {
    println!(
        r#"Push a local environment to the registry.

Usage: asterai env push <name> [options]

Arguments:
  <name>              Local environment name (e.g., my-env or namespace:my-env)

Options:
  -h, --help          Show this help message

Examples:
  asterai env push my-env
  asterai env push myteam:production-env
  asterai env push my-env --staging
"#
    );
}
