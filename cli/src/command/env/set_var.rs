use crate::command::env::EnvArgs;
use crate::command::resource_or_id::ResourceOrIdArg;
use crate::local_store::LocalStore;
use asterai_runtime::resource::ResourceId;
use eyre::{OptionExt, bail};
use std::str::FromStr;

#[derive(Debug)]
pub(super) struct SetVarArgs {
    /// Environment reference (namespace:name or just name for local).
    env_ref: String,
    /// Variable assignments (KEY=VALUE or KEY= to unset).
    vars: Vec<(String, Option<String>)>,
}

impl SetVarArgs {
    pub fn parse(mut args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut env_ref: Option<String> = None;
        let mut vars: Vec<(String, Option<String>)> = Vec::new();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--var" | "-v" => {
                    let var_string = args.next().ok_or_eyre("missing value for var flag")?;
                    let (key, value) = parse_var_assignment(&var_string)?;
                    vars.push((key, value));
                }
                "--help" | "-h" | "help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if env_ref.is_some() {
                        // Could be a var assignment without --var flag
                        if other.contains('=') {
                            let (key, value) = parse_var_assignment(other)?;
                            vars.push((key, value));
                        } else {
                            bail!("unexpected argument: {}", other);
                        }
                    } else {
                        env_ref = Some(other.to_string());
                    }
                }
            }
        }
        let env_ref = env_ref.ok_or_eyre(
            "missing environment reference\n\nUsage: asterai env set-var <name> --var KEY=VALUE\n\
             Example: asterai env set-var my-env --var API_KEY=secret",
        )?;
        if vars.is_empty() {
            bail!("no variables specified. Use --var KEY=VALUE to set a variable.");
        }
        Ok(Self { env_ref, vars })
    }

    pub fn execute(&self) -> eyre::Result<()> {
        let arg = ResourceOrIdArg::from_str(&self.env_ref).unwrap();
        let ns = arg.resolved_namespace();
        let id_string = format!("{ns}:{}", arg.name());
        let resource_id = ResourceId::from_str(&id_string)
            .map_err(|e| eyre::eyre!("invalid environment reference: {e}"))?;
        let mut environment = LocalStore::fetch_environment(&resource_id)?;
        println!(
            "updating environment {}:{}@{}",
            environment.namespace(),
            environment.name(),
            environment.version()
        );
        // Apply variable changes.
        for (key, value) in &self.vars {
            match value {
                Some(v) => {
                    environment.set_var(key.clone(), v.clone());
                    println!("  set {}={}", key, mask_value(v));
                }
                None => {
                    if environment.vars.remove(key).is_some() {
                        println!("  unset {}", key);
                    } else {
                        println!("  {} (not set, skipping)", key);
                    }
                }
            }
        }
        // Save the environment.
        LocalStore::write_environment(&environment)?;
        println!("saved");
        println!("\nNote: push the environment to apply changes to the registry:");
        println!(
            "  asterai env push {}:{}",
            environment.namespace(),
            environment.name()
        );
        Ok(())
    }
}

/// Parse a variable assignment like "KEY=VALUE" or "KEY=" (to unset).
fn parse_var_assignment(s: &str) -> eyre::Result<(String, Option<String>)> {
    let Some((key, value)) = s.split_once('=') else {
        bail!(
            "invalid variable format '{}': use KEY=VALUE or KEY= to unset",
            s
        );
    };
    if key.is_empty() {
        bail!("variable name cannot be empty");
    }
    // Validate key format (alphanumeric + underscore, starting with letter or underscore)
    if !key.chars().next().unwrap().is_ascii_alphabetic() && !key.starts_with('_') {
        bail!(
            "variable name '{}' must start with a letter or underscore",
            key
        );
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!(
            "variable name '{}' can only contain letters, numbers, and underscores",
            key
        );
    }
    let value = match value.is_empty() {
        true => None,
        false => Some(value.to_string()),
    };
    Ok((key.to_string(), value))
}

/// Mask a value for display (show first 4 chars if long enough).
fn mask_value(value: &str) -> String {
    if value.len() <= 8 {
        "*".repeat(value.len())
    } else {
        format!("{}...", &value[..4])
    }
}

fn print_help() {
    println!(
        r#"Set environment variables in a local environment.

Usage: asterai env set-var <name> [options] [KEY=VALUE...]

Arguments:
  <name>              Environment name (e.g., my-env or namespace:my-env)

Options:
  -v, --var KEY=VALUE Set a variable (can be repeated)
  -h, --help          Show this help message

To unset a variable, use KEY= (empty value):
  asterai env set-var my-env --var OLD_KEY=

Examples:
  asterai env set-var my-env --var API_KEY=secret
  asterai env set-var my-env --var DB_URL=postgres://... --var LOG_LEVEL=debug
  asterai env set-var my-env API_KEY=secret LOG_LEVEL=info
  asterai env set-var my-env --var OLD_VAR=          # Unset OLD_VAR

Note: After setting variables, push the environment to apply changes:
  asterai env push namespace:my-env
"#
    );
}

impl EnvArgs {
    pub fn set_var(&self) -> eyre::Result<()> {
        let args = self.set_var_args.as_ref().ok_or_eyre("no set-var args")?;
        args.execute()
    }
}
