use crate::config::CONFIG_DIR;
use std::fs;
use tokio::io;

pub fn read_stored_api_key() -> Option<String> {
    let path = CONFIG_DIR.join("auth");
    fs::read_to_string(path).ok()
}

pub fn store_api_key(value: &str) -> io::Result<()> {
    fs::create_dir_all(&*CONFIG_DIR)?;
    let path = CONFIG_DIR.join("auth");
    fs::write(path, value)
}

pub fn clear_api_key() -> io::Result<()> {
    let path = CONFIG_DIR.join("auth");
    fs::remove_file(path)
}
