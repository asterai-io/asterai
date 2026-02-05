use crate::config::{API_URL, API_URL_STAGING, REGISTRY_URL, REGISTRY_URL_STAGING};
use eyre::OptionExt;

// For local development.
pub const DEFAULT_SPECIFIC_ENDPOINT: &str = "http://localhost:3003";

/// Extract common flags (-e/--endpoint, -r/--registry, -s/--staging) from args.
/// Returns (api_endpoint, registry_endpoint, remaining_args).
/// If --staging is set, it overrides the endpoints with staging URLs.
pub fn extract_common_flags(args: Vec<String>) -> eyre::Result<(String, String, Vec<String>)> {
    let mut api_endpoint = API_URL.to_string();
    let mut registry_endpoint = REGISTRY_URL.to_string();
    let mut staging = false;
    let mut filtered = Vec::new();
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
            _ => filtered.push(arg),
        }
    }
    if staging {
        api_endpoint = API_URL_STAGING.to_string();
        registry_endpoint = REGISTRY_URL_STAGING.to_string();
    }
    Ok((api_endpoint, registry_endpoint, filtered))
}
