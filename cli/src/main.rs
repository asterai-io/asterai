use crate::command::Command;

pub mod auth;
pub mod cli_ext;
mod command;
pub mod config;
pub mod registry;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = std::env::args().skip(1);
    let command = match Command::parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to parse command: {e}");
            std::process::exit(1);
        }
    };
    match command.run().await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("error: {e:#?}");
            std::process::exit(1);
        }
    }
}
