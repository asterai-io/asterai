use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use eyre::{Context, OptionExt, bail};
use std::fs;
use std::path::{Path, PathBuf};

const BASE_API_URL: &str = "https://api.asterai.io";
const BASE_API_URL_STAGING: &str = "https://staging.api.asterai.io";
const RETRY_FIND_FILE_DIR: &str = "build/";
const COMPONENT_PUSH_HELP: &str = include_str!("../../../help/component_push.txt");

#[derive(Debug)]
pub(super) struct PushArgs {
    /// Path to component.wasm (optional for interface-only components).
    component: Option<String>,
    /// Path to package.wasm (WIT interface, required).
    pkg: String,
    endpoint: String,
    staging: bool,
    /// If true, only push the WIT interface (no component implementation).
    interface_only: bool,
}

impl PushArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut component: Option<String> = None;
        let mut pkg = "package.wasm".to_string();
        let mut endpoint = BASE_API_URL.to_string();
        let mut staging = false;
        let mut interface_only = false;
        let mut did_specify_pkg = false;
        let print_help_and_exit = || {
            println!("{COMPONENT_PUSH_HELP}");
            std::process::exit(0);
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--endpoint" | "-e" => {
                    endpoint = args.next().ok_or_eyre("missing value for endpoint flag")?;
                }
                "--staging" | "-s" => {
                    staging = true;
                }
                "--component" | "-c" => {
                    component = Some(args.next().ok_or_eyre("missing value for component flag")?);
                }
                "--pkg" => {
                    pkg = args.next().ok_or_eyre("missing value for pkg flag")?;
                    did_specify_pkg = true;
                }
                "--interface-only" | "-i" => {
                    interface_only = true;
                }
                "--help" | "-h" | "help" => {
                    print_help_and_exit();
                }
                _ => bail!("unknown flag: {}", arg),
            }
        }
        // Read package (WIT interface) - required.
        let does_pkg_exist = check_does_file_exist(&pkg);
        if !does_pkg_exist && !did_specify_pkg {
            print_help_and_exit();
        }
        // If not explicitly interface-only, try to find component.wasm.
        if !interface_only && component.is_none() {
            // Try default paths.
            if check_does_file_exist("component.wasm") {
                component = Some("component.wasm".to_string());
            } else {
                // No component file found and not interface-only.
                // Proceed without it (treat as interface-only).
            }
        }
        Ok(Self {
            component,
            pkg,
            endpoint,
            staging,
            interface_only,
        })
    }

    async fn execute_push(&self) -> eyre::Result<()> {
        let api_key = Auth::read_stored_api_key().ok_or_eyre("API key not found")?;
        let client = reqwest::Client::new();
        let pkg_bytes = read_file(&self.pkg)?;
        // Build multipart form.
        let mut form = reqwest::multipart::Form::new().part(
            "package.wasm",
            reqwest::multipart::Part::bytes(pkg_bytes)
                .file_name("package.wasm")
                .mime_str("application/octet-stream")?,
        );
        // Add component if provided and not interface-only.
        let is_interface_only = self.interface_only || self.component.is_none();
        if let Some(ref component_path) = self.component {
            if !self.interface_only {
                let component_bytes = read_file(component_path)?;
                form = form.part(
                    "component.wasm",
                    reqwest::multipart::Part::bytes(component_bytes)
                        .file_name("component.wasm")
                        .mime_str("application/octet-stream")?,
                );
            }
        }
        // Determine base URL.
        let base_url = if self.staging {
            BASE_API_URL_STAGING
        } else {
            &self.endpoint
        };
        if is_interface_only {
            println!("pushing WIT interface (interface-only)...");
        } else {
            println!("pushing component with WIT interface...");
        }
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
            bail!("push failed ({}): {}", status, error_text);
        }
        println!("done");
        Ok(())
    }
}

fn check_does_file_exist(relative_path: &str) -> bool {
    let path = Path::new(relative_path);
    if path.exists() {
        return true;
    }
    let retry_path = PathBuf::from(RETRY_FIND_FILE_DIR).join(relative_path);
    retry_path.exists()
}

fn read_file(relative_path: &str) -> eyre::Result<Vec<u8>> {
    let path = Path::new(relative_path);
    if path.exists() {
        return fs::read(path).wrap_err_with(|| format!("failed to read file: {}", relative_path));
    }
    // Retry with build/ directory.
    let retry_path = PathBuf::from(RETRY_FIND_FILE_DIR).join(relative_path);
    if retry_path.exists() {
        return fs::read(&retry_path)
            .wrap_err_with(|| format!("failed to read file: {:?}", retry_path));
    }
    bail!(
        "file not found: {} (also tried {})",
        relative_path,
        retry_path.display()
    )
}

impl ComponentArgs {
    pub async fn push(&self) -> eyre::Result<()> {
        let args = self.push_args.as_ref().ok_or_eyre("no push args")?;
        args.execute_push().await
    }
}
