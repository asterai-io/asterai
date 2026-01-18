use crate::cli_ext::component::ComponentCliExt;
use crate::command::component::ComponentArgs;
use asterai_runtime::component::interface::ComponentInterface;

impl ComponentArgs {
    pub fn list(&self) -> eyre::Result<()> {
        let mut output = String::new();
        let components = ComponentInterface::local_list();
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
