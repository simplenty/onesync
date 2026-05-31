use crate::{
    config::{ConfigEdit, OneDriveConfig},
    utils::{config_root, expand_home, unix_timestamp},
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountStatus {
    NeedsAuth,
    Authenticating,
    Authenticated,
    Syncing,
    Monitoring,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub config_dir: String,
    pub sync_dir: String,
    pub status: AccountStatus,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AccountStore {
    pub accounts: Vec<Account>,
}

pub fn load_store() -> io::Result<AccountStore> {
    let path = store_path();
    if !path.exists() {
        return Ok(AccountStore::default());
    }
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(io::Error::other)
}

pub fn save_accounts(accounts: &[Account]) -> io::Result<()> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let store = AccountStore {
        accounts: accounts.to_vec(),
    };
    let content = serde_json::to_string_pretty(&store).map_err(io::Error::other)?;
    fs::write(path, content)
}

pub fn create_account(
    existing: &[Account],
    name: &str,
    email: &str,
    sync_dir: &str,
) -> io::Result<Account> {
    validate_unique(existing, name, sync_dir)?;
    let name = if name.is_empty() { "OneDrive" } else { name };
    let id = format!("{}-{}", sanitize_id(name), unix_timestamp());
    let config_dir = profiles_root().join(&id);
    fs::create_dir_all(&config_dir)?;

    let mut config = OneDriveConfig::default();
    config.apply_edit(&ConfigEdit {
        sync_dir: sync_dir.to_string(),
        ..ConfigEdit::default()
    });
    config.write_with_backup(config_dir.join("config"))?;
    fs::create_dir_all(expand_home(sync_dir))?;

    Ok(Account {
        id,
        name: name.to_string(),
        email: email.to_string(),
        config_dir: config_dir.to_string_lossy().to_string(),
        sync_dir: sync_dir.to_string(),
        status: AccountStatus::NeedsAuth,
    })
}

pub fn is_authenticated(account: &Account) -> bool {
    Path::new(&account.config_dir)
        .join("refresh_token")
        .exists()
}

pub fn auth_url_path(account: &Account) -> PathBuf {
    Path::new(&account.config_dir).join("auth-url")
}

pub fn auth_response_path(account: &Account) -> PathBuf {
    Path::new(&account.config_dir).join("auth-response")
}

pub fn suggested_account_name() -> String {
    let count = load_store()
        .map(|store| store.accounts.len() + 1)
        .unwrap_or(1);
    format!("OneDrive {count}")
}

pub fn suggested_sync_dir() -> String {
    let count = load_store()
        .map(|store| store.accounts.len() + 1)
        .unwrap_or(1);
    if count == 1 {
        "~/OneDrive".to_string()
    } else {
        format!("~/OneDrive-{count}")
    }
}

fn validate_unique(existing: &[Account], name: &str, sync_dir: &str) -> io::Result<()> {
    if existing.iter().any(|account| account.name == name) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "profile name already exists",
        ));
    }
    let sync_dir = expand_home(sync_dir);
    if existing
        .iter()
        .any(|account| expand_home(&account.sync_dir) == sync_dir)
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "sync directory already exists",
        ));
    }
    Ok(())
}

fn store_path() -> PathBuf {
    config_root().join("accounts.json")
}

fn profiles_root() -> PathBuf {
    config_root().join("profiles")
}

fn sanitize_id(value: &str) -> String {
    let mut id = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
        } else if character == '-' || character == '_' || character.is_whitespace() {
            id.push('-');
        }
    }
    let trimmed = id.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "onedrive".to_string()
    } else {
        trimmed
    }
}

pub fn remove_confirmation_matches(expected_name: &str, input: &str) -> bool {
    input == expected_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_account_sync_directory() {
        let existing = Account {
            id: "existing".to_string(),
            name: "Existing".to_string(),
            email: String::new(),
            config_dir: "/tmp/existing".to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        };

        let error = create_account(&[existing], "Imported", "", "~/OneDrive")
            .expect_err("duplicate sync_dir should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn account_store_ignores_unknown_future_fields() {
        let json = r#"{
            "schema_version": 2,
            "accounts": [
                {
                    "id": "personal",
                    "name": "Personal",
                    "email": "person@example.com",
                    "config_dir": "/tmp/onedrive",
                    "sync_dir": "~/OneDrive",
                    "status": "Authenticated",
                    "future_field": "ignored"
                }
            ]
        }"#;

        let store: AccountStore = serde_json::from_str(json).unwrap();

        assert_eq!(store.accounts.len(), 1);
        assert_eq!(store.accounts[0].id, "personal");
        assert_eq!(store.accounts[0].status, AccountStatus::Authenticated);
    }

    #[test]
    fn remove_profile_requires_exact_current_name() {
        assert!(remove_confirmation_matches("Work Drive", "Work Drive"));
        assert!(!remove_confirmation_matches("Work Drive", "work drive"));
        assert!(!remove_confirmation_matches("Work Drive", " Work Drive "));
        assert!(!remove_confirmation_matches("Work Drive", "Personal"));
    }
}
