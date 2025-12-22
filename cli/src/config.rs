use once_cell::sync::Lazy;
use std::env;
use std::path::PathBuf;

const APP_DIR_NAME: &str = "asterai";

pub static CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let home_dir = if cfg!(windows) {
        env::var("LOCALAPPDATA").unwrap_or_else(|_| env::var("USERPROFILE").unwrap())
    } else {
        env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", env::var("HOME").unwrap()))
    };
    PathBuf::from(home_dir).join(APP_DIR_NAME)
});

pub static BIN_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let bin_dir = if cfg!(windows) {
        env::var("LOCALAPPDATA").unwrap_or_else(|_| env::var("USERPROFILE").unwrap())
    } else {
        env::var("XDG_DATA_HOME")
            .unwrap_or_else(|_| format!("{}/.local/bin", env::var("HOME").unwrap()))
    };
    PathBuf::from(bin_dir).join(APP_DIR_NAME).join("bin")
});
