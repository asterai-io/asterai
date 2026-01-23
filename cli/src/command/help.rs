const HELP_STR: &str = include_str!("../../help/help.txt");

pub struct Help;

impl Help {
    pub fn execute() -> eyre::Result<()> {
        println!("{HELP_STR}");
        Ok(())
    }
}
