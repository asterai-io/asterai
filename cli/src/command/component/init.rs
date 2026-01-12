use crate::command::component::ComponentArgs;
use eyre::{Context, OptionExt, bail};
use include_dir::{Dir, include_dir};
use std::fs;
use std::path::Path;

static TYPESCRIPT_TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/init/typescript");
static RUST_TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/init/rust");

#[derive(Debug)]
pub(super) struct InitArgs {
    out_dir: String,
    rust: bool,
    typescript: bool,
}

impl InitArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut out_dir = None;
        let mut rust = false;
        let mut typescript = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--rust" => {
                    rust = true;
                }
                "--typescript" => {
                    typescript = true;
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
            out_dir: out_dir.unwrap_or_else(|| "plugin".to_string()),
            rust,
            typescript,
        })
    }

    fn execute_init(&self) -> eyre::Result<()> {
        // Validate flags
        self.validate_language_flags()?;

        // Get the template
        let template = if self.rust {
            &RUST_TEMPLATE
        } else {
            // Typescript is default
            &TYPESCRIPT_TEMPLATE
        };

        // Resolve output directory
        let out_dir = fs::canonicalize(".")
            .wrap_err("failed to get current directory")?
            .join(&self.out_dir);

        if out_dir.exists() {
            bail!("output directory already exists: {:?}", out_dir);
        }

        // Extract template to output directory
        extract_template(template, &out_dir)
            .wrap_err_with(|| format!("failed to extract template to {:?}", out_dir))?;

        println!("Initialized plugin project at {:?}", out_dir);
        Ok(())
    }

    fn validate_language_flags(&self) -> eyre::Result<()> {
        let flags = [self.rust, self.typescript];
        let true_count = flags.iter().filter(|&&f| f).count();

        if true_count == 0 && !self.typescript {
            // No flags set, typescript is default
            return Ok(());
        }

        if true_count > 1 {
            bail!("only one language flag can be set");
        }

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

    // Extract all files from the embedded directory
    for file in template.files() {
        let file_path = dst.join(file.path());
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .wrap_err_with(|| format!("failed to create parent directory: {:?}", parent))?;
        }
        fs::write(&file_path, file.contents())
            .wrap_err_with(|| format!("failed to write file: {:?}", file_path))?;
    }

    // Extract all directories
    for dir in template.dirs() {
        let dir_path = dst.join(dir.path());
        fs::create_dir_all(&dir_path)
            .wrap_err_with(|| format!("failed to create directory: {:?}", dir_path))?;
    }

    Ok(())
}
