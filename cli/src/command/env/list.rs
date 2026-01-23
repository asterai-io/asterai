use crate::auth::Auth;
use crate::command::env::EnvArgs;
use crate::config::API_URL;
use crate::local_store::LocalStore;
use eyre::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEnvironmentsResponse {
    environments: Vec<EnvironmentSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentSummary {
    namespace: String,
    name: String,
    latest_version: String,
}

impl EnvArgs {
    pub async fn list(&self) -> eyre::Result<()> {
        // Show local environments.
        let envs = LocalStore::list_environments();
        println!("local environments:");
        if envs.is_empty() {
            println!("  (none)");
        } else {
            for env in envs {
                println!(
                    "  {}  ({} components)",
                    env.resource_ref(),
                    env.components.len()
                );
            }
        }
        // Show remote environments if authenticated.
        println!();
        if let Some(api_key) = Auth::read_stored_api_key() {
            match fetch_remote_environments(&api_key).await {
                Ok(remote_envs) => {
                    println!("remote environments:");
                    if remote_envs.is_empty() {
                        println!("  (none)");
                    } else {
                        for env in remote_envs {
                            println!("  {}:{}@{}", env.namespace, env.name, env.latest_version);
                        }
                    }
                }
                Err(e) => {
                    println!("remote environments:");
                    println!("  (failed to fetch: {})", e);
                }
            }
        } else {
            println!("remote environments:");
            println!("  (not authenticated - run 'asterai auth login')");
        }

        Ok(())
    }
}

async fn fetch_remote_environments(api_key: &str) -> eyre::Result<Vec<EnvironmentSummary>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/v1/environments", API_URL))
        .header("Authorization", api_key.trim())
        .send()
        .await
        .wrap_err("failed to fetch environments")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        eyre::bail!("{}: {}", status, error_text);
    }

    let result: ListEnvironmentsResponse =
        response.json().await.wrap_err("failed to parse response")?;

    Ok(result.environments)
}
