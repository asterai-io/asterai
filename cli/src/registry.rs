use crate::auth::Auth;
use crate::config::ARTIFACTS_DIR;
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

/// OCI referrers index response.
#[derive(Deserialize)]
pub struct OciIndex {
    pub manifests: Vec<OciIndexEntry>,
}

#[derive(Deserialize)]
pub struct OciIndexEntry {
    pub digest: String,
    #[serde(rename = "artifactType")]
    pub artifact_type: Option<String>,
}

/// OCI tag list response.
#[derive(Deserialize)]
pub struct TagListResponse {
    pub tags: Vec<String>,
}

const ARTIFACT_TYPE_WIT: &str = "application/vnd.wasm.wit.v1+wasm";

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
    /// If `api_key` is `None`, uses stored API key if available.
    pub async fn get_token(&self, api_key: Option<&str>, repo_name: &str) -> eyre::Result<String> {
        let scope = format!("repository:{}:pull", repo_name);
        let token_url = format!("{}/v1/registry/token?scope={}", self.api_url, scope);
        let mut request = self.client.get(&token_url);
        // Use provided API key, or fall back to stored API key.
        let effective_key = api_key
            .map(|k| k.to_string())
            .or_else(Auth::read_stored_api_key);
        if let Some(key) = effective_key {
            request = request.header("Authorization", format!("Bearer {}", key.trim()));
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
    /// Returns the manifest and its digest (from Docker-Content-Digest header).
    pub async fn fetch_manifest(
        &self,
        repo_name: &str,
        tag: &str,
        token: &str,
    ) -> eyre::Result<(OciManifest, String)> {
        let manifest_url = format!("{}/v2/{}/manifests/{}", self.registry_url, repo_name, tag);
        let response = self
            .client
            .get(&manifest_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.oci.image.manifest.v1+json")
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
        let digest = response
            .headers()
            .get("Docker-Content-Digest")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let manifest: OciManifest = response.json().await.wrap_err("failed to parse manifest")?;
        Ok((manifest, digest))
    }

    /// Download a blob from the registry.
    pub async fn download_blob(
        &self,
        repo_name: &str,
        digest: &str,
        token: &str,
    ) -> eyre::Result<Vec<u8>> {
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
        let bytes = response
            .bytes()
            .await
            .wrap_err("failed to read blob bytes")?;
        Ok(bytes.to_vec())
    }

    /// Fetch the WIT package.wasm for a component via the OCI referrers API,
    /// falling back to the OCI 1.1 tag-based referrers index if the native
    /// API is not supported (404).
    pub async fn fetch_wit_referrer(
        &self,
        repo_name: &str,
        manifest_digest: &str,
        token: &str,
    ) -> eyre::Result<Option<Vec<u8>>> {
        if manifest_digest.is_empty() {
            return Ok(None);
        }
        // Try native referrers API first.
        let index = self
            .fetch_referrers_native(repo_name, manifest_digest, token)
            .await?;
        // Fall back to tag-based index.
        let index = match index {
            Some(idx) => idx,
            None => {
                match self
                    .fetch_referrers_tag(repo_name, manifest_digest, token)
                    .await?
                {
                    Some(idx) => idx,
                    None => return Ok(None),
                }
            }
        };
        let wit_entry = index.manifests.iter().find(|m| {
            m.artifact_type
                .as_deref()
                .is_some_and(|t| t == ARTIFACT_TYPE_WIT)
        });
        let Some(entry) = wit_entry else {
            return Ok(None);
        };
        let (wit_manifest, _) = self.fetch_manifest(repo_name, &entry.digest, token).await?;
        let Some(layer) = wit_manifest.layers.first() else {
            return Ok(None);
        };
        let blob = self.download_blob(repo_name, &layer.digest, token).await?;
        Ok(Some(blob))
    }

    /// Try the native OCI referrers API. Returns None if not supported (404).
    async fn fetch_referrers_native(
        &self,
        repo_name: &str,
        manifest_digest: &str,
        token: &str,
    ) -> eyre::Result<Option<OciIndex>> {
        let url = format!(
            "{}/v2/{}/referrers/{}?artifactType={}",
            self.registry_url, repo_name, manifest_digest, ARTIFACT_TYPE_WIT
        );
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.oci.image.index.v1+json")
            .send()
            .await
            .wrap_err("failed to fetch referrers")?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let index: OciIndex = response
            .json()
            .await
            .wrap_err("failed to parse referrers index")?;
        Ok(Some(index))
    }

    /// Fetch the tag-based referrers index (OCI 1.1 fallback).
    /// Tag format: `sha256-<hex>` (subject digest with `:` replaced by `-`).
    async fn fetch_referrers_tag(
        &self,
        repo_name: &str,
        manifest_digest: &str,
        token: &str,
    ) -> eyre::Result<Option<OciIndex>> {
        let fallback_tag = manifest_digest.replace(':', "-");
        let url = format!(
            "{}/v2/{}/manifests/{}",
            self.registry_url, repo_name, fallback_tag
        );
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.oci.image.index.v1+json")
            .send()
            .await
            .wrap_err("failed to fetch referrers tag index")?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let index: OciIndex = response
            .json()
            .await
            .wrap_err("failed to parse referrers tag index")?;
        Ok(Some(index))
    }

    /// List tags for a repository in the OCI registry.
    pub async fn list_tags(
        &self,
        api_key: Option<&str>,
        repo_name: &str,
    ) -> eyre::Result<Vec<String>> {
        let token = self.get_token(api_key, repo_name).await?;
        let url = format!("{}/v2/{}/tags/list", self.registry_url, repo_name);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .wrap_err("failed to list tags")?;
        if !response.status().is_success() {
            return Ok(vec![]);
        }
        let result: TagListResponse = response.json().await.wrap_err("failed to parse tag list")?;
        Ok(result.tags)
    }

    /// Pull a component from the registry and save it locally.
    /// If `api_key` is `None`, uses stored API key if available.
    /// Returns the output directory path.
    pub async fn pull_component(
        &self,
        api_key: Option<&str>,
        component: &Component,
        quiet: bool,
    ) -> eyre::Result<PathBuf> {
        let namespace = component.namespace();
        let name = component.name();
        let version = component.version().to_string();
        let repo_name = format!("{}/{}", namespace, name);
        // Check if component already exists locally.
        let output_dir = ARTIFACTS_DIR
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
        let (manifest, manifest_digest) = self.fetch_manifest(&repo_name, &version, &token).await?;
        // Create output directory.
        fs::create_dir_all(&output_dir)?;
        // Download layers.
        for (i, layer) in manifest.layers.iter().enumerate() {
            let blob_bytes = self
                .download_blob(&repo_name, &layer.digest, &token)
                .await?;
            let filename = match i {
                0 => "component.wasm",
                _ => "package.wasm",
            };
            let file_path = output_dir.join(filename);
            fs::write(&file_path, &blob_bytes)?;
        }
        // Fetch WIT package via referrers API.
        if let Ok(Some(wit_bytes)) = self
            .fetch_wit_referrer(&repo_name, &manifest_digest, &token)
            .await
        {
            fs::write(output_dir.join("package.wasm"), &wit_bytes)?;
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
