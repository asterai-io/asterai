use eyre::{Context, Report, eyre};
use log::info;
use std::fs;
use std::str::FromStr;
use tempfile::{tempdir, tempdir_in};
use thiserror::Error;
use wasm_pkg_client::caching::{CachingClient, FileCache};
use wasm_pkg_client::oci::{BasicCredentials, OciRegistryConfig};
use wasm_pkg_common::config::{Config, RegistryMapping};
use wasm_pkg_common::label::Label;
use wasm_pkg_core::lock::{LockFile, LockedPackage};
use wasm_pkg_core::wit;
use wasm_pkg_core::wit::OutputType;

#[derive(Error, Debug)]
pub enum BuildComponentWitPkgError {
    #[error("Internal error: {0:#?}")]
    Internal(#[from] Report),
    #[error("Failed to fetch dependencies: {0}")]
    FetchDependencies(Report),
}

// TODO: add access control to prevent users from downloading private packages
// from others.
// This is done such that users can't activate private components into agents,
// but with the `pkg` command they can currently fetch private component
// interfaces / WITs (though not the component implementation).
pub async fn build_component_wit_pkg(
    pkg_wit_bytes: &[u8],
    wkg_client: wasm_pkg_client::Client,
) -> Result<Vec<u8>, BuildComponentWitPkgError> {
    // Initialise temp directories and write package file.
    let pkg_dir = tempdir().map_err(|e| eyre!(e))?;
    let cache_dir = tempdir_in(&pkg_dir).map_err(|e| eyre!(e))?;
    let pkg_file_path = pkg_dir.path().join("package.wit");
    fs::write(pkg_file_path, pkg_wit_bytes).map_err(|e| eyre!(e))?;
    let cache = FileCache::new(&cache_dir)
        .await
        .map_err(|e| eyre!(e))
        .with_context(|| "failed to create new wit cache")?;
    let wkg_caching_client = CachingClient::new(Some(wkg_client), cache);
    let wkg_config = wasm_pkg_core::config::Config::default();
    let locked_packages = Vec::<LockedPackage>::new();
    let lock_file_path = pkg_dir.path().join("wkg.lock");
    let mut lock_file = LockFile::new_with_path(locked_packages, lock_file_path)
        .await
        .map_err(|e| eyre!(e))
        .with_context(|| "failed to get new wit lock file")?;
    // Fetch dependencies & build package.
    wit::fetch_dependencies(
        &wkg_config,
        &pkg_dir,
        &mut lock_file,
        wkg_caching_client.clone(),
        OutputType::Wit,
    )
    .await
    .map_err(|e| eyre!(e))
    .map_err(BuildComponentWitPkgError::FetchDependencies)?;
    let (pkg_ref, version, pkg_bin_bytes) =
        wit::build_package(&wkg_config, &pkg_dir, &mut lock_file, wkg_caching_client)
            .await
            .map_err(|e| eyre!(e))
            .with_context(|| "failed to build wit package")?;
    let version_string = match version {
        None => "n/a".to_owned(),
        Some(v) => format!("@{v}"),
    };
    info!(
        "built component wit pkg bin: {pkg_ref}{version_string} ({} bytes)",
        pkg_bin_bytes.len()
    );
    Ok(pkg_bin_bytes)
}

pub fn new_wkg_client(
    registry_url: &str,
    registry_credentials: BasicCredentials,
) -> eyre::Result<wasm_pkg_client::Client> {
    // Set & configure the default OCI registry.
    let mut config = Config::empty();
    let registry =
        wasm_pkg_common::registry::Registry::from_str(registry_url).map_err(|e| eyre!(e))?;
    let oci_config = OciRegistryConfig {
        client_config: Default::default(),
        credentials: Some(registry_credentials),
    };
    config.set_default_registry(Some(registry.clone()));
    // Set the default registry for the wasi namespace.
    config.set_namespace_registry(
        Label::from_str("wasi").map_err(|e| eyre!(e))?,
        RegistryMapping::Registry(
            wasm_pkg_common::registry::Registry::from_str("wasi.dev").map_err(|e| eyre!(e))?,
        ),
    );
    let registry_config = config.get_or_insert_registry_config_mut(&registry);
    registry_config
        .set_backend_config("oci", &oci_config)
        .with_context(|| "failed to set oci backend config")?;
    let wkg_client = wasm_pkg_client::Client::new(config);
    Ok(wkg_client)
}
