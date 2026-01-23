use crate::auth::Auth;
use crate::config::BIN_DIR;
use asterai_runtime::component::Component;
use asterai_runtime::resource::metadata::ResourceKind;
use eyre::{Context, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Token response from the registry auth endpoint.
#[derive(Deserialize)]
pub struct TokenResponse {
    pub token: String,
    #[allow(dead_code)]
    pub expires_in: u64,
    #[allow(dead_code)]
    pub issued_at: i64,
}

/// OCI image manifest structure.
#[derive(Deserialize)]
pub struct OciManifest {
    #[serde(rename = "schemaVersion")]
    #[allow(dead_code)]
    pub schema_version: u32,
    #[serde(rename = "mediaType")]
    #[allow(dead_code)]
    pub media_type: Option<String>,
    #[allow(dead_code)]
    pub config: OciDescriptor,
    pub layers: Vec<OciDescriptor>,
}

/// OCI content descriptor.
#[derive(Deserialize)]
pub struct OciDescriptor {
    #[serde(rename = "mediaType")]
    #[allow(dead_code)]
    pub media_type: String,
    pub digest: String,
    #[allow(dead_code)]
    pub size: u64,
}

/// Response from getting an environment from the API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetEnvironmentResponse {
    pub namespace: String,
    pub name: String,
    pub version: String,
    pub components: Vec<String>,
    pub vars: HashMap<String, String>,
}

/// Client for interacting with the OCI registry.
pub struct RegistryClient<'a> {
    client: &'a reqwest::Client,
    api_url: &'a str,
    registry_url: &'a str,
}

impl<'a> RegistryClient<'a> {
    pub fn new(client: &'a reqwest::Client, api_url: &'a str, registry_url: &'a str) -> Self {
        Self {
            client,
            api_url,
            registry_url,
        }
    }

    /// Get a registry token for the given repository and scope.
    pub async fn get_token(&self, api_key: &str, repo_name: &str) -> eyre::Result<String> {
        let scope = format!("repository:{}:pull", repo_name);
        let token_url = format!("{}/v1/registry/token?scope={}", self.api_url, scope);
        let response = self
            .client
            .get(&token_url)
            .header("Authorization", format!("Bearer {}", api_key.trim()))
            .send()
            .await
            .wrap_err("failed to get registry token")?;
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to get registry token ({}): {}", status, error_text);
        }
        let token_response: TokenResponse = response
            .json()
            .await
            .wrap_err("failed to parse token response")?;
        Ok(token_response.token)
    }

    /// Get a registry token, using stored API key if available.
    pub async fn get_token_optional_auth(&self, repo_name: &str) -> eyre::Result<String> {
        let scope = format!("repository:{}:pull", repo_name);
        let token_url = format!("{}/v1/registry/token?scope={}", self.api_url, scope);
        let mut request = self.client.get(&token_url);
        if let Some(api_key) = Auth::read_stored_api_key() {
            request = request.header("Authorization", format!("Bearer {}", api_key.trim()));
        }
        let response = request
            .send()
            .await
            .wrap_err("failed to get registry token")?;
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to get registry token ({}): {}", status, error_text);
        }
        let token_response: TokenResponse = response
            .json()
            .await
            .wrap_err("failed to parse token response")?;
        Ok(token_response.token)
    }

    /// Fetch an OCI manifest for the given repository and tag.
    pub async fn fetch_manifest(
        &self,
        repo_name: &str,
        tag: &str,
        token: &str,
    ) -> eyre::Result<OciManifest> {
        let manifest_url = format!("{}/v2/{}/manifests/{}", self.registry_url, repo_name, tag);
        let response = self
            .client
            .get(&manifest_url)
            .header("Authorization", format!("Bearer {}", token))
            .header(
                "Accept",
                "application/vnd.oci.image.manifest.v1+json, \
                application/vnd.docker.distribution.manifest.v2+json",
            )
            .send()
            .await
            .wrap_err("failed to fetch manifest")?;
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to fetch manifest ({}): {}", status, error_text);
        }
        let manifest: OciManifest = response.json().await.wrap_err("failed to parse manifest")?;
        Ok(manifest)
    }

    /// Download a blob from the registry.
    pub async fn download_blob(&self, repo_name: &str, digest: &str, token: &str) -> eyre::Result<Vec<u8>> {
        let blob_url = format!("{}/v2/{}/blobs/{}", self.registry_url, repo_name, digest);
        let response = self
            .client
            .get(&blob_url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .wrap_err("failed to fetch blob")?;
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            bail!("failed to fetch blob ({}): {}", status, error_text);
        }
        let bytes = response.bytes().await.wrap_err("failed to read blob bytes")?;
        Ok(bytes.to_vec())
    }

    /// Pull a component from the registry and save it locally.
    /// Returns the output directory path.
    pub async fn pull_component(
        &self,
        api_key: &str,
        component: &Component,
        quiet: bool,
    ) -> eyre::Result<PathBuf> {
        let namespace = component.namespace();
        let name = component.name();
        let version = component.version().to_string();
        let repo_name = format!("{}/{}", namespace, name);
        // Check if component already exists locally.
        let output_dir = BIN_DIR
            .join("resources")
            .join(namespace)
            .join(format!("{}@{}", name, version));
        if output_dir.exists() {
            if !quiet {
                println!("  {}@{} (cached)", repo_name, version);
            }
            return Ok(output_dir);
        }
        if !quiet {
            println!("  pulling {}@{}...", repo_name, version);
        }
        // Get registry token.
        let token = self.get_token(api_key, &repo_name).await?;
        // Fetch manifest.
        let manifest = self.fetch_manifest(&repo_name, &version, &token).await?;
        // Create output directory.
        fs::create_dir_all(&output_dir)?;
        // Download layers.
        for (i, layer) in manifest.layers.iter().enumerate() {
            let blob_bytes = self.download_blob(&repo_name, &layer.digest, &token).await?;
            let filename = if i == 0 {
                "component.wasm"
            } else {
                "package.wasm"
            };
            let file_path = output_dir.join(filename);
            fs::write(&file_path, &blob_bytes)?;
        }
        // Write component metadata.
        let metadata = serde_json::json!({
            "kind": ResourceKind::Component.to_string(),
            "pulled_from": format!("{}@{}", repo_name, version),
        });
        let metadata_path = output_dir.join("metadata.json");
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;
        Ok(output_dir)
    }
}
