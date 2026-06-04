use crate::{sync::SyncMode, utils::config_root};
use serde_json::{Map, Value};
use std::{fs, io};

pub const DEFAULT_ONEDRIVE_COMMAND: &str = "onedrive";
const PROFILE_SYNC_MODES_KEY: &str = "profile_sync_modes";

pub fn load_onedrive_command() -> io::Result<String> {
    let value = load_settings_value()?;
    Ok(value
        .get("onedrive_binary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|binary| !binary.is_empty())
        .unwrap_or(DEFAULT_ONEDRIVE_COMMAND)
        .to_string())
}

pub fn load_profile_sync_mode(profile_id: &str) -> io::Result<SyncMode> {
    let value = load_settings_value()?;
    Ok(value
        .get(PROFILE_SYNC_MODES_KEY)
        .and_then(Value::as_object)
        .and_then(|modes| modes.get(profile_id))
        .and_then(sync_mode_from_value)
        .unwrap_or(SyncMode::Manual))
}

pub fn save_profile_sync_mode(profile_id: &str, mode: SyncMode) -> io::Result<()> {
    let mut value = load_settings_value()?;
    let root = ensure_object(&mut value);
    let modes = root
        .entry(PROFILE_SYNC_MODES_KEY)
        .or_insert_with(|| Value::Object(Map::new()));
    let modes = ensure_object(modes);
    modes.insert(profile_id.to_string(), sync_mode_to_value(mode));
    save_settings_value(&value)
}

pub fn remove_profile_sync_mode(profile_id: &str) -> io::Result<()> {
    let mut value = load_settings_value()?;
    if let Some(modes) = value
        .get_mut(PROFILE_SYNC_MODES_KEY)
        .and_then(Value::as_object_mut)
    {
        modes.remove(profile_id);
    }
    save_settings_value(&value)
}

fn load_settings_value() -> io::Result<Value> {
    let path = config_root().join("settings.json");
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }

    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(io::Error::other)
}

fn save_settings_value(value: &Value) -> io::Result<()> {
    let path = config_root().join("settings.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    fs::write(path, content)
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("value was just made an object")
}

fn sync_mode_from_value(value: &Value) -> Option<SyncMode> {
    match value.as_str()? {
        "manual" => Some(SyncMode::Manual),
        "automatic" => Some(SyncMode::Automatic),
        _ => None,
    }
}

fn sync_mode_to_value(mode: SyncMode) -> Value {
    match mode {
        SyncMode::Manual => Value::String("manual".to_string()),
        SyncMode::Automatic => Value::String("automatic".to_string()),
    }
}
