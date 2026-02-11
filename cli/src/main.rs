use crate::command::Command;
pub mod artifact;
pub mod auth;
mod command;
pub mod config;
pub mod editor;
pub mod language;
pub mod local_store;
pub mod registry;
pub mod runtime;
pub mod version_resolver;

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
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
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}
