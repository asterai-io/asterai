const HELP_STR: &str = include_str!("../../help.txt");

pub struct Help;

impl Help {
    pub fn run() -> eyre::Result<()> {
        println!("{HELP_STR}");
        Ok(())
    }
}
