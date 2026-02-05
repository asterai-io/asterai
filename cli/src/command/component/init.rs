use crate::auth::Auth;
use crate::command::component::ComponentArgs;
use crate::language;
use eyre::{Context, OptionExt, bail};
use include_dir::Dir;
use std::fs;
use std::path::Path;

const DEFAULT_LANGUAGE: &str = "typescript";

#[derive(Debug)]
pub(super) struct InitArgs {
    out_dir: String,
    language: String,
}

impl InitArgs {
    pub fn parse(args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut out_dir = None;
        let mut language = None;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-l" | "--language" => {
                    let value = args.next().ok_or_else(|| {
                        eyre::eyre!(
                            "--language requires a value. \
                             Supported: {}",
                            language::supported_names()
                        )
                    })?;
                    language = Some(value);
                }
                _ => {
                    if out_dir.is_none() {
                        out_dir = Some(arg);
                    } else {
                        bail!("unexpected argument: {}", arg);
                    }
                }
            }
        }
        Ok(Self {
            out_dir: out_dir.unwrap_or_else(|| "component".to_string()),
            language: language.unwrap_or_else(|| DEFAULT_LANGUAGE.to_string()),
        })
    }

    fn execute_init(&self) -> eyre::Result<()> {
        validate_wit_identifier(&self.out_dir)?;
        let language = language::from_name(&self.language).ok_or_else(|| {
            eyre::eyre!(
                "unsupported language: '{}'. Supported: {}",
                self.language,
                language::supported_names()
            )
        })?;
        // Resolve output directory.
        let out_dir = fs::canonicalize(".")
            .wrap_err("failed to get current directory")?
            .join(&self.out_dir);
        if out_dir.exists() {
            bail!("output directory already exists: {:?}", out_dir);
        }
        let namespace =
            Auth::read_stored_user_namespace().unwrap_or_else(|| "your-username".to_string());
        // Extract template to output directory.
        extract_template(language.template(), &out_dir, &namespace, &self.out_dir)
            .wrap_err_with(|| format!("failed to extract template to {:?}", out_dir))?;
        println!(
            "Initialized {} component project at {:?}",
            language.name(),
            out_dir
        );
        Ok(())
    }
}

impl ComponentArgs {
    pub fn init(&self) -> eyre::Result<()> {
        let args = self.init_args.as_ref().ok_or_eyre("no init args")?;
        args.execute_init()
    }
}

fn extract_template(
    template: &Dir,
    dst: &Path,
    namespace: &str,
    component_name: &str,
) -> eyre::Result<()> {
    let namespace_snake = namespace.replace('-', "_");
    fs::create_dir_all(dst).wrap_err_with(|| format!("failed to create directory: {:?}", dst))?;
    for file in template.files() {
        let mut file_path = dst.join(file.path());
        // Rename .template files back to their original names.
        // (Cargo excludes dirs with Cargo.toml, so we use .template extension.)
        if file_path.extension().is_some_and(|ext| ext == "template") {
            file_path.set_extension("");
        }
        let contents = match std::str::from_utf8(file.contents()) {
            Ok(text) => text
                .replace("___USERNAME___", namespace)
                .replace("___USERNAME_SNAKE___", &namespace_snake)
                .replace("___COMPONENT___", component_name)
                .into_bytes(),
            Err(_) => file.contents().to_vec(),
        };
        fs::write(&file_path, contents)
            .wrap_err_with(|| format!("failed to write file: {:?}", file_path))?;
    }
    for dir in template.dirs() {
        let dir_path = dst.join(dir.path());
        fs::create_dir_all(&dir_path)
            .wrap_err_with(|| format!("failed to create directory: {:?}", dir_path))?;
        extract_template(dir, dst, namespace, component_name)?;
    }
    Ok(())
}

/// Validates that a name is a valid WIT identifier (kebab-case).
fn validate_wit_identifier(name: &str) -> eyre::Result<()> {
    if name.is_empty() {
        bail!("component name cannot be empty");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("component name cannot start or end with a hyphen: \"{name}\"");
    }
    if name.contains("--") {
        bail!("component name cannot contain consecutive hyphens: \"{name}\"");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!(
            "component name must be lowercase alphanumeric with hyphens (kebab-case): \"{name}\""
        );
    }
    Ok(())
}
