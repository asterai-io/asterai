use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use eyre::{Context, OptionExt, bail};
use std::fs;
use std::path::{Path, PathBuf};

const BASE_API_URL: &str = "https://api.asterai.io";
const BASE_API_URL_STAGING: &str = "https://staging.api.asterai.io";
const RETRY_FIND_FILE_DIR: &str = "build/";

#[derive(Debug)]
pub(super) struct PushArgs {
    component: String,
    pkg: String,
    endpoint: String,
    staging: bool,
}

impl PushArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut component = "component.wasm".to_string();
        let mut pkg = "package.wasm".to_string();
        let mut endpoint = BASE_API_URL.to_string();
        let mut staging = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-e" | "--endpoint" => {
                    endpoint = args.next().ok_or_eyre("missing value for endpoint flag")?;
                }
                "-s" | "--staging" => {
                    staging = true;
                }
                "--component" => {
                    component = args.next().ok_or_eyre("missing value for component flag")?;
                }
                "--pkg" => {
                    pkg = args.next().ok_or_eyre("missing value for pkg flag")?;
                }
                _ => bail!("unknown flag: {}", arg),
            }
        }
        Ok(Self {
            component,
            pkg,
            endpoint,
            staging,
        })
    }

    async fn execute_push(&self) -> eyre::Result<()> {
        let api_key = Auth::read_stored_api_key().ok_or_eyre("API key not found")?;
        let client = reqwest::Client::new();

        // Read files with retry logic
        let component_bytes = read_file(&self.component)?;
        let pkg_bytes = read_file(&self.pkg)?;

        // Build multipart form
        let mut form = reqwest::multipart::Form::new()
            .part(
                "component.wasm",
                reqwest::multipart::Part::bytes(component_bytes)
                    .file_name("component.wasm")
                    .mime_str("application/octet-stream")?,
            )
            .part(
                "package.wasm",
                reqwest::multipart::Part::bytes(pkg_bytes)
                    .file_name("package.wasm")
                    .mime_str("application/octet-stream")?,
            );
        // Determine base URL
        let base_url = if self.staging {
            BASE_API_URL_STAGING
        } else {
            &self.endpoint
        };
        // Send request
        let response = client
            .put(format!("{}/v1/component", base_url))
            .header("Authorization", api_key.trim())
            .multipart(form)
            .send()
            .await
            .wrap_err("failed to send push request")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("request error: {}", error_text);
        }

        println!("done");
        Ok(())
    }
}

fn read_file(relative_path: &str) -> eyre::Result<Vec<u8>> {
    let path = Path::new(relative_path);

    if path.exists() {
        return fs::read(path).wrap_err_with(|| format!("failed to read file: {}", relative_path));
    }

    // Retry with build/ directory
    let retry_path = PathBuf::from(RETRY_FIND_FILE_DIR).join(relative_path);
    if retry_path.exists() {
        return fs::read(&retry_path)
            .wrap_err_with(|| format!("failed to read file: {:?}", retry_path));
    }

    bail!("file not found: {}", relative_path)
}

impl ComponentArgs {
    pub async fn push(&self) -> eyre::Result<()> {
        let args = self.push_args.as_ref().ok_or_eyre("no push args")?;
        args.execute_push().await
    }
}
