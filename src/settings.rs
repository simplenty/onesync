use crate::utils::config_root;
use std::{fs, io};

pub const DEFAULT_ONEDRIVE_COMMAND: &str = "onedrive";

pub fn load_onedrive_command() -> io::Result<String> {
    let path = config_root().join("settings.json");
    if !path.exists() {
        return Ok(DEFAULT_ONEDRIVE_COMMAND.to_string());
    }

    let content = fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&content).map_err(io::Error::other)?;
    Ok(value
        .get("onedrive_binary")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|binary| !binary.is_empty())
        .unwrap_or(DEFAULT_ONEDRIVE_COMMAND)
        .to_string())
}
