use crate::command::Command;

pub mod auth;
pub mod cli_ext;
mod command;
pub mod config;

#[tokio::main]
async fn main() {
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
