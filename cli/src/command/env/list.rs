use crate::command::env::{EnvArgs, EnvironmentCliExt};
use asterai_runtime::environment::Environment;

impl EnvArgs {
    pub fn list(&self) -> eyre::Result<()> {
        let mut output = String::new();
        let local_envs = Environment::local_list();
        output.push_str("local environments:\n");
        for env_path in local_envs {
            let Ok(env) = Environment::parse_local(&env_path) else {
                eprintln!(
                    "ERROR: failed to parse environment at {}",
                    env_path.to_str().unwrap_or_default()
                );
                continue;
            };
            let line = format!(
                " - {env_resource}: {plugin_count} components\n",
                env_resource = env.resource(),
                plugin_count = env.plugins().len()
            );
            output.push_str(&line);
        }
        println!("{output}");
        Ok(())
    }
}
