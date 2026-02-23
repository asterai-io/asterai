use once_cell::sync::Lazy;
use std::env;
use std::path::PathBuf;

pub const API_URL: &str = "https://api.asterai.io";
pub const API_URL_STAGING: &str = "https://staging.api.asterai.io";
pub const REGISTRY_URL: &str = "https://registry.asterai.io";
pub const REGISTRY_URL_STAGING: &str = "https://staging.registry.asterai.io";

pub static BASE_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let home = if cfg!(windows) {
        env::var("USERPROFILE").unwrap()
    } else {
        env::var("HOME").unwrap()
    };
    PathBuf::from(home).join(".asterai")
});

pub static CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| BASE_DIR.join("config"));

pub static BIN_DIR: Lazy<PathBuf> = Lazy::new(|| BASE_DIR.join("bin"));

/// Directory for storing artifacts (environments, components).
pub static ARTIFACTS_DIR: Lazy<PathBuf> = Lazy::new(|| BIN_DIR.join("artifacts"));
