use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use crate::language;
use eyre::{Context, OptionExt, bail};
use reqwest::StatusCode;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
const RETRY_FIND_FILE_DIR: &str = "build/";
const COMPONENT_PUSH_HELP: &str = include_str!("../../../help/component_push.txt");

/// Structured error from server when version already exists.
#[derive(Deserialize)]
struct VersionConflictError {
    error: String,
    message: String,
    version: String,
    /// If true, CLI should auto-bump the version.
    /// If false, CLI should just display the error (user can use --force).
    can_auto_bump: bool,
}

#[derive(Debug)]
pub(super) struct PushArgs {
    /// Path to component.wasm (optional for interface-only components).
    component: Option<String>,
    /// Path to package.wasm (WIT interface, required).
    pkg: String,
    /// If true, only push the WIT interface (no component implementation).
    interface_only: bool,
    /// Force overwrite existing version (for private mutable versions).
    force: bool,
    /// Push (and create) the component as public.
    public: bool,
}

impl PushArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut component: Option<String> = None;
        let mut pkg: Option<String> = None;
        let mut interface_only = false;
        let mut force = false;
        let mut public = false;
        let print_help_and_exit = || {
            println!("{COMPONENT_PUSH_HELP}");
            std::process::exit(0);
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--component" | "-c" => {
                    component = Some(args.next().ok_or_eyre("missing value for component flag")?);
                }
                "--pkg" => {
                    pkg = Some(args.next().ok_or_eyre("missing value for pkg flag")?);
                }
                "--interface-only" | "-i" => {
                    interface_only = true;
                }
                "--force" | "-f" => {
                    force = true;
                }
                "--public" | "-p" => {
                    public = true;
                }
                "--help" | "-h" | "help" => {
                    print_help_and_exit();
                }
                _ => bail!("unknown flag: {}", arg),
            }
        }
        // Try language detection if no explicit paths provided.
        let cwd = std::env::current_dir().wrap_err("failed to get current directory")?;
        if let Some(lang) = language::detect(&cwd) {
            if pkg.is_none() {
                let lang_pkg_path = lang.get_package_wasm_path(&cwd);
                if lang_pkg_path.exists() {
                    pkg = Some(lang_pkg_path.to_string_lossy().to_string());
                }
            }
            if component.is_none()
                && !interface_only
                && let Ok(lang_component_path) = lang.get_component_wasm_path(&cwd)
                && lang_component_path.exists()
            {
                component = Some(lang_component_path.to_string_lossy().to_string());
            }
        }
        // Fall back to default paths if not found via language detection.
        let pkg = match pkg {
            Some(p) => p,
            None => {
                if check_does_file_exist("package.wasm") {
                    "package.wasm".to_string()
                } else {
                    print_help_and_exit();
                    unreachable!()
                }
            }
        };
        // If not explicitly interface-only, try to find component.wasm.
        if !interface_only && component.is_none() && check_does_file_exist("component.wasm") {
            component = Some("component.wasm".to_string());
        }
        Ok(Self {
            component,
            pkg,
            interface_only,
            force,
            public,
        })
    }

    async fn execute_push(&self, api_endpoint: &str) -> eyre::Result<()> {
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
        if let Some(ref component_path) = self.component
            && !self.interface_only
        {
            let component_bytes = read_file(component_path)?;
            form = form.part(
                "component.wasm",
                reqwest::multipart::Part::bytes(component_bytes)
                    .file_name("component.wasm")
                    .mime_str("application/octet-stream")?,
            );
        }
        if self.force {
            form = form.text("force", "true");
        }
        if self.public {
            form = form.text("public", "true");
        }
        if is_interface_only {
            println!("pushing WIT interface (interface-only)...");
        } else {
            println!("pushing component with WIT interface...");
        }
        let response = client
            .put(format!("{}/v1/component", api_endpoint))
            .header("Authorization", api_key.trim())
            .multipart(form)
            .send()
            .await
            .wrap_err("failed to send push request")?;
        let status = response.status();
        // Handle version conflict (409 Conflict).
        if status == StatusCode::CONFLICT {
            let body = response.text().await?;
            if let Ok(err) = serde_json::from_str::<VersionConflictError>(&body)
                && err.error == "version_exists"
            {
                if err.can_auto_bump {
                    // Public immutable version: auto-bump and instruct to rebuild.
                    return self.handle_version_conflict(&err.version);
                } else {
                    // Mutable version or private: just show the error message.
                    bail!("{}", err.message);
                }
            }
            bail!("push failed ({}): {}", status, body);
        }
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

    /// Handles a version conflict by auto-bumping the version in the WIT file
    /// and instructing the user to rebuild.
    fn handle_version_conflict(&self, current_version: &str) -> eyre::Result<()> {
        // Cannot auto-bump :latest.
        if current_version == "latest" {
            bail!(
                "Version :latest already exists but cannot be auto-bumped.\n\
                 Add a semver version to your WIT package declaration."
            );
        }
        // Bump patch version.
        let new_version = bump_patch_version(current_version)?;
        // Update WIT file.
        let wit_path = self.update_wit_version(current_version, &new_version)?;
        // Instruct user to rebuild and retry.
        println!("Version {} already exists in registry.", current_version);
        println!(
            "Bumped version in {}: {} -> {}",
            wit_path.display(),
            current_version,
            new_version
        );
        println!("\nPlease rebuild your component and push again:");
        println!("  cargo component build --release");
        println!("  asterai component push");
        Ok(())
    }

    /// Updates the version in the WIT file.
    fn update_wit_version(&self, old_version: &str, new_version: &str) -> eyre::Result<PathBuf> {
        // Find WIT files in the project.
        let wit_dir = Path::new("wit");
        if !wit_dir.exists() {
            bail!("wit/ directory not found. Cannot auto-bump version.");
        }
        // Look for the package declaration with the old version.
        for entry in fs::read_dir(wit_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wit") {
                let content = fs::read_to_string(&path)?;
                if content.contains(&format!("@{}", old_version)) {
                    let new_content =
                        content.replace(&format!("@{}", old_version), &format!("@{}", new_version));
                    fs::write(&path, &new_content)?;
                    return Ok(path);
                }
            }
        }
        bail!("Could not find version {} in WIT files", old_version);
    }
}

/// Bumps the patch version of a semver string.
/// Only called for immutable release versions (X.Y.Z) that triggered a 409.
/// Pre-release versions are mutable and never trigger 409, so no special handling needed.
fn bump_patch_version(version: &str) -> eyre::Result<String> {
    let semver =
        semver::Version::parse(version).wrap_err_with(|| format!("Invalid semver: {}", version))?;
    Ok(format!(
        "{}.{}.{}",
        semver.major,
        semver.minor,
        semver.patch + 1
    ))
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
        args.execute_push(&self.api_endpoint).await
    }
}
