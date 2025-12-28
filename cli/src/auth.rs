use crate::config::CONFIG_DIR;
use once_cell::sync::Lazy;
use std::fs;
use std::path::PathBuf;
use tokio::io;

pub static LOCAL_NAMESPACE: &str = "local";

static AUTH_FILE_PATH: Lazy<PathBuf> = Lazy::new(|| CONFIG_DIR.join("auth"));
/// Path to the file storing the local namespace preference.
static USER_NAMESPACE_FILE_PATH: Lazy<PathBuf> = Lazy::new(|| CONFIG_DIR.join("namespace"));

pub struct Auth;

impl Auth {
    pub fn read_stored_api_key() -> Option<String> {
        fs::read_to_string(&*AUTH_FILE_PATH).ok()
    }

    pub fn store_api_key(value: &str) -> io::Result<()> {
        fs::create_dir_all(&*CONFIG_DIR)?;
        fs::write(&*AUTH_FILE_PATH, value)
    }

    pub fn clear_api_key() -> io::Result<()> {
        fs::remove_file(&*AUTH_FILE_PATH)
    }

    pub fn read_stored_user_namespace() -> Option<String> {
        fs::read_to_string(&*USER_NAMESPACE_FILE_PATH).ok()
    }

    pub fn read_user_or_fallback_namespace() -> String {
        Auth::read_stored_user_namespace().unwrap_or_else(|| LOCAL_NAMESPACE.to_owned())
    }
}
