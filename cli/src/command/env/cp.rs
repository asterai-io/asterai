use crate::auth::Auth;
use crate::local_store::LocalStore;
use asterai_runtime::environment::Environment;
use asterai_runtime::resource::ResourceId;
use eyre::{OptionExt, bail};
use std::str::FromStr;

#[derive(Debug)]
pub struct CpArgs {
    source: String,
    dest: String,
}

impl CpArgs {
    pub fn parse(args: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let mut source: Option<String> = None;
        let mut dest: Option<String> = None;
        for arg in args {
            match arg.as_str() {
                "--help" | "-h" | "help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    if other.starts_with('-') {
                        bail!("unknown flag: {}", other);
                    }
                    if source.is_none() {
                        source = Some(other.to_string());
                    } else if dest.is_none() {
                        dest = Some(other.to_string());
                    } else {
                        bail!("unexpected argument: {}", other);
                    }
                }
            }
        }
        let source = source.ok_or_eyre(
            "missing source environment\n\nUsage: asterai env cp <source> <dest>\n\
             Example: asterai env cp my-env namespace:my-env",
        )?;
        let dest = dest.ok_or_eyre(
            "missing destination environment\n\nUsage: asterai env cp <source> <dest>\n\
             Example: asterai env cp my-env namespace:my-env",
        )?;
        Ok(Self { source, dest })
    }

    pub fn execute(&self) -> eyre::Result<()> {
        // Parse source, falling back to local namespace.
        let source_id = ResourceId::from_str(&self.source).or_else(|_| {
            let with_namespace = format!("local:{}", self.source);
            ResourceId::from_str(&with_namespace)
        })?;
        // Parse destination, falling back to user namespace.
        let dest_id = ResourceId::from_str(&self.dest).or_else(|_| {
            let with_namespace =
                format!("{}:{}", Auth::read_user_or_fallback_namespace(), self.dest);
            ResourceId::from_str(&with_namespace)
        })?;
        // Fetch source environment (try exact match, then local namespace).
        let source_env = LocalStore::fetch_environment(&source_id).or_else(|_| {
            let local_id =
                ResourceId::new_from_parts("local".to_string(), source_id.name().to_string())?;
            LocalStore::fetch_environment(&local_id)
        });
        let source_env = source_env
            .map_err(|_| eyre::eyre!("source environment '{}' not found", self.source))?;
        // Check if destination already exists.
        if LocalStore::fetch_environment(&dest_id).is_ok() {
            bail!(
                "destination environment '{}:{}' already exists\n\
                 hint: delete it first with: asterai env delete {}:{}",
                dest_id.namespace(),
                dest_id.name(),
                dest_id.namespace(),
                dest_id.name()
            );
        }
        // Create new environment with destination namespace/name and version 0.0.0.
        let mut new_env = Environment::new(
            dest_id.namespace().to_string(),
            dest_id.name().to_string(),
            "0.0.0".to_string(),
        );
        new_env.components = source_env.components.clone();
        new_env.vars = source_env.vars.clone();
        // Write to local storage.
        LocalStore::write_environment(&new_env)?;
        println!(
            "copied {} -> {}",
            source_env.display_ref(),
            new_env.resource_id()
        );
        Ok(())
    }
}

fn print_help() {
    println!(
        r#"Copy an environment to a new namespace:name.

This creates a local copy of an environment. The copied environment is unpushed
and can be modified or pushed to the registry separately.

Usage: asterai env cp <source> <dest>

Arguments:
  <source>    Source environment (e.g., my-env or namespace:my-env)
  <dest>      Destination environment (e.g., new-name or namespace:new-name)

Examples:
  asterai env cp my-env lorenzo:my-env      # Copy local:my-env to lorenzo:my-env
  asterai env cp local:test prod:test       # Copy with explicit namespaces
  asterai env cp myteam:staging myteam:prod # Copy between names in same namespace
"#
    );
}
