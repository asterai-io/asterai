use crate::command::component::ComponentArgs;
use crate::local_store::LocalStore;

impl ComponentArgs {
    pub fn list(&self) -> eyre::Result<()> {
        let mut output = String::new();
        let components = LocalStore::list_components();
        output.push_str("local components:\n");
        for component in components {
            let line = format!(
                " - {name}: {function_count} functions\n",
                name = component.component(),
                function_count = component.get_functions().len()
            );
            output.push_str(&line);
        }
        println!("{output}");
        Ok(())
    }
}
