use crate::config::{API_URL, API_URL_STAGING, REGISTRY_URL, REGISTRY_URL_STAGING};
use crate::runtime::expand_tilde;
use eyre::OptionExt;
use std::path::PathBuf;

// For local development.
pub const DEFAULT_SPECIFIC_ENDPOINT: &str = "http://localhost:3003";

/// Extracted common flags shared across subcommands.
pub struct CommonFlags {
    pub api_endpoint: String,
    pub registry_endpoint: String,
    pub allow_dirs: Vec<PathBuf>,
    pub remaining_args: Vec<String>,
}

/// Extract common flags from args.
/// Returns CommonFlags with endpoints, allowed dirs, and remaining args.
/// If --staging is set, it overrides the endpoints with staging URLs.
pub fn extract_common_flags(args: Vec<String>) -> eyre::Result<CommonFlags> {
    let mut api_endpoint = API_URL.to_string();
    let mut registry_endpoint = REGISTRY_URL.to_string();
    let mut staging = false;
    let mut allow_dirs = Vec::new();
    let mut filtered = Vec::new();
    let home = std::env::var("HOME").ok();
    let mut iter = args.into_iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--endpoint" | "-e" => {
                api_endpoint = iter
                    .next()
                    .unwrap_or_else(|| DEFAULT_SPECIFIC_ENDPOINT.to_owned());
            }
            "--registry" | "-r" => {
                registry_endpoint = iter
                    .next()
                    .ok_or_eyre("missing value for --registry flag")?;
            }
            "--staging" | "-s" => {
                staging = true;
            }
            "--allow-dir" => {
                let dir = iter
                    .next()
                    .ok_or_eyre("missing value for --allow-dir flag")?;
                allow_dirs.push(expand_tilde(&dir, home.as_deref()));
            }
            _ => filtered.push(arg),
        }
    }
    if staging {
        api_endpoint = API_URL_STAGING.to_string();
        registry_endpoint = REGISTRY_URL_STAGING.to_string();
    }
    Ok(CommonFlags {
        api_endpoint,
        registry_endpoint,
        allow_dirs,
        remaining_args: filtered,
    })
}
