use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use eyre::{Context, OptionExt, bail, eyre};
use std::fs;
use std::path::{Path, PathBuf};

const BASE_API_URL: &str = "https://api.asterai.io";

pub(super) struct PkgArgs {
    wit_input_path: String,
    endpoint: String,
    output: String,
    wit: Option<String>,
}

impl PkgArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let wit_input_path = args.next().unwrap_or_else(|| "plugin.wit".to_string());
        let mut endpoint = BASE_API_URL.to_string();
        let mut output = "package.wasm".to_string();
        let mut wit = Some("package.wit".to_string());
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-o" | "--output" => {
                    output = args.next().ok_or_eyre("missing value for output flag")?;
                }
                "-w" | "--wit" => {
                    wit = Some(args.next().ok_or_eyre("missing value for wit flag")?);
                }
                "-e" | "--endpoint" => {
                    endpoint = args.next().ok_or_eyre("missing value for endpoint flag")?;
                }
                _ => bail!("unknown flag: {}", arg),
            }
        }
        Ok(Self {
            wit_input_path,
            endpoint,
            output,
            wit,
        })
    }
}

impl ComponentArgs {
    pub async fn pkg(&self) -> eyre::Result<()> {
        let args = self.pkg_args.as_ref().ok_or_eyre("no pkg args")?;
        let wit_path = PathBuf::from(&args.wit_input_path);
        let wit_path = fs::canonicalize(&wit_path)
            .wrap_err_with(|| format!("WIT file not found at {:?}", wit_path))?;
        if !wit_path.exists() {
            bail!("WIT file not found at {:?}", wit_path);
        }
        let base_dir = wit_path
            .parent()
            .ok_or_eyre("failed to get parent directory")?;
        let output_file = base_dir.join(&args.output);
        // Read the WIT file
        let wit_content = fs::read(&wit_path)
            .wrap_err_with(|| format!("failed to read WIT file at {:?}", wit_path))?;
        // Create multipart form
        let api_key = Auth::read_stored_api_key().ok_or_eyre("API key not found")?;
        let client = reqwest::Client::new();
        let form = reqwest::multipart::Form::new().part(
            "package.wit",
            reqwest::multipart::Part::bytes(wit_content)
                .file_name("package.wit")
                .mime_str("application/octet-stream")?,
        );
        // Send request to /v1/pkg
        let response = client
            .post(format!("{}/v1/pkg", args.endpoint))
            .header("Authorization", api_key.trim())
            .multipart(form)
            .send()
            .await
            .wrap_err("failed to send request to pkg endpoint")?;
        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("request failed: {}", error_text.replace("\\n", "\n"));
        }
        let package_bytes = response
            .bytes()
            .await
            .wrap_err("failed to read response body")?;
        // Write output file
        fs::write(&output_file, package_bytes)
            .wrap_err_with(|| format!("failed to write output file to {:?}", output_file))?;
        println!("Package created at {:?}", output_file);
        // Convert to WIT if requested
        if let Some(wit_output) = &args.wit {
            wasm2wit(&args.endpoint, &output_file, &base_dir.join(wit_output)).await?;
        }
        Ok(())
    }
}

async fn wasm2wit(endpoint: &str, input_file: &Path, output_file: &Path) -> eyre::Result<()> {
    let wasm_content = fs::read(input_file)
        .wrap_err_with(|| format!("failed to read WASM file at {:?}", input_file))?;
    let api_key = Auth::read_stored_api_key().ok_or_eyre("API key not found")?;
    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new().part(
        "package.wasm",
        reqwest::multipart::Part::bytes(wasm_content)
            .file_name("package.wasm")
            .mime_str("application/octet-stream")?,
    );
    let response = client
        .post(format!("{}/v1/wasm2wit", endpoint))
        .header("Authorization", api_key.trim())
        .multipart(form)
        .send()
        .await
        .wrap_err("failed to send request to wasm2wit endpoint")?;
    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        bail!("request failed: {}", error_text);
    }
    let wit_text = response
        .text()
        .await
        .wrap_err("failed to read response body")?;
    fs::write(output_file, wit_text)
        .wrap_err_with(|| format!("failed to write WIT file to {:?}", output_file))?;
    println!("WIT file created at {:?}", output_file);
    Ok(())
}
