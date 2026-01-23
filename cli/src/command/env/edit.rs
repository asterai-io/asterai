use crate::command::env::EnvArgs;
use crate::editor::open_in_editor;
use crate::local_store::LocalStore;

impl EnvArgs {
    pub fn edit(&self) -> eyre::Result<()> {
        let resource_id = self.resource_id()?;
        let env = LocalStore::fetch_environment(&resource_id)?;
        let env_dir = LocalStore::environment_dir(&env);
        let env_file = env_dir.join("env.toml");
        if !env_file.exists() {
            eyre::bail!("environment file not found: {}", env_file.display());
        }
        open_in_editor(&env_file)
    }
}
