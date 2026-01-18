use crate::cli_ext::environment::EnvironmentCliExt;
use crate::command::env::EnvArgs;
use asterai_runtime::environment::Environment;

impl EnvArgs {
    pub fn list(&self) -> eyre::Result<()> {
        let mut output = String::new();
        let envs = Environment::local_list();
        output.push_str("local environments:\n");
        for env in envs {
            let line = format!(
                " - {env_resource}: {component_count} components\n",
                env_resource = env.resource,
                component_count = env.components.len()
            );
            output.push_str(&line);
        }
        println!("{output}");
        Ok(())
    }
}
