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
        // Extract template to output directory.
        extract_template(language.template(), &out_dir)
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

fn extract_template(template: &Dir, dst: &Path) -> eyre::Result<()> {
    fs::create_dir_all(dst).wrap_err_with(|| format!("failed to create directory: {:?}", dst))?;
    for file in template.files() {
        let mut file_path = dst.join(file.path());
        // Rename .template files back to their original names.
        // (Cargo excludes dirs with Cargo.toml, so we use .template extension.)
        if file_path.extension().is_some_and(|ext| ext == "template") {
            file_path.set_extension("");
        }
        fs::write(&file_path, file.contents())
            .wrap_err_with(|| format!("failed to write file: {:?}", file_path))?;
    }
    for dir in template.dirs() {
        let dir_path = dst.join(dir.path());
        fs::create_dir_all(&dir_path)
            .wrap_err_with(|| format!("failed to create directory: {:?}", dir_path))?;
        extract_template(dir, dst)?;
    }
    Ok(())
}
