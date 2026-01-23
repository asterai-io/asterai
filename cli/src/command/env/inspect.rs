use crate::command::env::EnvArgs;
use crate::local_store::LocalStore;

impl EnvArgs {
    pub fn inspect(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let Ok(env) = LocalStore::fetch_environment(&resource_id) else {
            println!("environment does not exist");
            return Ok(());
        };
        println!(
            "environment {} has {} components",
            env.resource_ref(),
            env.components.len()
        );
        if env.components.is_empty() {
            println!("components: (none)");
            return Ok(());
        }
        let mut components: Vec<_> = env.component_refs();
        components.sort();
        println!("components:");
        for component in components {
            println!(" - {component}");
        }
        if !env.vars.is_empty() {
            println!("vars:");
            let mut vars: Vec<_> = env.vars.keys().collect();
            vars.sort();
            for var in vars {
                println!(" - {var}");
            }
        }
        Ok(())
    }
}
