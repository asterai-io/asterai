const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Version;

impl Version {
    pub fn execute() -> eyre::Result<()> {
        println!("{VERSION}");
        Ok(())
    }
}
