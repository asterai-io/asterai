use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct InspectData {
    pub display_ref: String,
    pub components: Vec<String>,
    pub vars: Vec<String>,
    pub var_values: HashMap<String, String>,
}

impl EnvArgs {
    pub fn inspect(&self) -> eyre::Result<()> {
        let data = self.inspect_data()?;
        let Some(data) = data else {
            println!("environment does not exist");
            return Ok(());
        };
        println!(
            "environment {} has {} components",
            data.display_ref,
            data.components.len()
        );
        if data.components.is_empty() {
            println!("components: (none)");
        } else {
            println!("components:");
            for component in &data.components {
                println!(" - {component}");
            }
        }
        if !data.vars.is_empty() {
            println!("vars:");
            for var in &data.vars {
                println!(" - {var}");
            }
        }
        Ok(())
    }

    /// Return structured inspect data.
    pub fn inspect_data(&self) -> eyre::Result<Option<InspectData>> {
        let resource_id = self.resource_id()?;
        let Ok(env) = LocalStore::fetch_environment(&resource_id) else {
            return Ok(None);
        };
        let mut components = env.component_refs();
        components.sort();
        let var_values: HashMap<String, String> = env
            .vars
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut vars: Vec<String> = var_values.keys().cloned().collect();
        vars.sort();
        Ok(Some(InspectData {
            display_ref: env.display_ref(),
            components,
            vars,
            var_values,
        }))
    }
}
