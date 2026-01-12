use crate::command::component::ComponentArgs;
use eyre::{bail, Context, OptionExt};
use std::fs;
use std::path::{Path, PathBuf};

const INIT_TYPESCRIPT_DIR: &str = "init/typescript";
const INIT_RUST_DIR: &str = "init/rust";

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

        // Determine source directory
        let source_dir = self.get_source_dir()?;

        // Resolve output directory
        let out_dir = fs::canonicalize(".")
            .wrap_err("failed to get current directory")?
            .join(&self.out_dir);

        if out_dir.exists() {
            bail!("output directory already exists: {:?}", out_dir);
        }

        // Copy template directory
        copy_dir_recursive(&source_dir, &out_dir)
            .wrap_err_with(|| format!("failed to copy template from {:?} to {:?}", source_dir, out_dir))?;

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

    fn get_source_dir(&self) -> eyre::Result<PathBuf> {
        // Get the executable's directory
        let exe_path = std::env::current_exe()
            .wrap_err("failed to get executable path")?;
        let exe_dir = exe_path.parent()
            .ok_or_eyre("failed to get executable directory")?;

        // Try relative to executable first (for installed binary)
        let template_dir = if self.rust {
            INIT_RUST_DIR
        } else {
            // Typescript is default
            INIT_TYPESCRIPT_DIR
        };

        let source_dir = exe_dir.join(template_dir);
        if source_dir.exists() {
            return Ok(source_dir);
        }

        // Try relative to current directory (for development)
        let source_dir = PathBuf::from("cli").join(template_dir);
        if source_dir.exists() {
            return Ok(source_dir);
        }

        // Try relative to workspace root
        let source_dir = PathBuf::from("..").join("cli").join(template_dir);
        if source_dir.exists() {
            return Ok(source_dir);
        }

        bail!("could not find template directory: {}", template_dir);
    }
}

impl ComponentArgs {
    pub fn init(&self) -> eyre::Result<()> {
        let args = self.init_args.as_ref().ok_or_eyre("no init args")?;
        args.execute_init()
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> eyre::Result<()> {
    fs::create_dir_all(dst)
        .wrap_err_with(|| format!("failed to create directory: {:?}", dst))?;

    for entry in fs::read_dir(src)
        .wrap_err_with(|| format!("failed to read directory: {:?}", src))?
    {
        let entry = entry.wrap_err("failed to read directory entry")?;
        let path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if path.is_dir() {
            copy_dir_recursive(&path, &dst_path)?;
        } else {
            fs::copy(&path, &dst_path)
                .wrap_err_with(|| format!("failed to copy file from {:?} to {:?}", path, dst_path))?;
        }
    }

    Ok(())
}
