//! Editor utilities for opening files in the user's preferred editor.
use std::path::Path;
use std::process::Command;

/// Get the user's preferred editor.
pub fn get_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        })
}

/// Open a file in the user's preferred editor.
pub fn open_in_editor(path: &Path) -> eyre::Result<()> {
    let editor = get_editor();
    let status = Command::new(&editor)
        .arg(path)
        .status()
        .map_err(|e| eyre::eyre!("failed to launch editor '{}': {}", editor, e))?;
    if !status.success() {
        eyre::bail!("editor '{}' exited with status: {}", editor, status);
    }
    Ok(())
}
