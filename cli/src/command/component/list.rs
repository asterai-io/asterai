use crate::cli_ext::component_binary::ComponentBinaryCliExt;
use crate::command::component::ComponentArgs;
use asterai_runtime::component::interface::ComponentBinary;

impl ComponentArgs {
    pub fn list(&self) -> eyre::Result<()> {
        let mut output = String::new();
        let components = ComponentBinary::local_list();
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
