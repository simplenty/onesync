use crate::event::BackendError;
use crate::{
    profile::config::{ConfigEdit, OneDriveConfig},
    utils::{config_root, expand_home, unix_timestamp},
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountStoreError {
    DuplicateAccountName,
    DuplicateSyncDir,
}

impl From<&AccountStoreError> for BackendError {
    fn from(error: &AccountStoreError) -> Self {
        match error {
            AccountStoreError::DuplicateAccountName => BackendError::DuplicateAccountName,
            AccountStoreError::DuplicateSyncDir => BackendError::DuplicateSyncDir,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountStatus {
    NeedsAuth,
    Authenticated,
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

impl AccountStore {
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn accounts_mut(&mut self) -> &mut Vec<Account> {
        &mut self.accounts
    }

    /// Create a new account, append it, and persist. Returns the new account.
    pub fn add(&mut self, name: &str, email: &str, sync_dir: &str) -> io::Result<Account> {
        let account = create_account(&self.accounts, name, email, sync_dir)
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
        self.accounts.push(account.clone());
        self.flush()?;
        Ok(account)
    }

    pub fn update_status(&mut self, account_id: &str, status: AccountStatus) -> io::Result<()> {
        if let Some(account) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            account.status = status;
            self.flush()?;
        }
        Ok(())
    }

    /// Returns true if any field changed. Applies the same name-replacement rule
    /// the app layer used (replace default/empty/email-derived names).
    pub fn update_identity(
        &mut self,
        account_id: &str,
        display_name: Option<&str>,
        email: Option<&str>,
    ) -> io::Result<bool> {
        let mut changed = false;
        if let Some(account) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            let should_replace_name = should_replace_profile_name(&account.name, &account.email);
            if let Some(email) = email
                && account.email != email
            {
                account.email = email.to_string();
                changed = true;
            }
            if let Some(display_name) = display_name
                && should_replace_name
                && account.name != display_name
            {
                account.name = display_name.to_string();
                changed = true;
            }
        }
        if changed {
            self.flush()?;
        }
        Ok(changed)
    }

    pub fn remove(&mut self, account_id: &str) -> io::Result<()> {
        self.accounts.retain(|a| a.id != account_id);
        self.flush()
    }

    pub fn flush(&self) -> io::Result<()> {
        save_accounts(&self.accounts)
    }
}

fn should_replace_profile_name(current_name: &str, current_email: &str) -> bool {
    let name = current_name.trim();
    name.is_empty() || name == "OneDrive" || name.starts_with("OneDrive ") || name == current_email
}

pub fn create_account(
    existing: &[Account],
    name: &str,
    email: &str,
    sync_dir: &str,
) -> Result<Account, AccountStoreError> {
    validate_unique(existing, name, sync_dir)?;
    let name = if name.is_empty() { "OneDrive" } else { name };
    let id = format!("{}-{}", sanitize_id(name), unix_timestamp());
    let config_dir = profiles_root().join(&id);
    fs::create_dir_all(&config_dir).expect("failed to create profile config directory");

    let mut config = OneDriveConfig::default();
    config.apply_edit(&ConfigEdit {
        sync_dir: sync_dir.to_string(),
        ..ConfigEdit::default()
    });
    config
        .write_with_backup(config_dir.join("config"))
        .expect("failed to write profile config");
    fs::create_dir_all(expand_home(sync_dir)).expect("failed to create sync directory");

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

fn validate_unique(
    existing: &[Account],
    name: &str,
    sync_dir: &str,
) -> Result<(), AccountStoreError> {
    if existing.iter().any(|account| account.name == name) {
        return Err(AccountStoreError::DuplicateAccountName);
    }
    let sync_dir = expand_home(sync_dir);
    if existing
        .iter()
        .any(|account| expand_home(&account.sync_dir) == sync_dir)
    {
        return Err(AccountStoreError::DuplicateSyncDir);
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

        assert_eq!(error, AccountStoreError::DuplicateSyncDir);
    }

    #[test]
    fn rejects_duplicate_account_name() {
        let existing = Account {
            id: "existing".to_string(),
            name: "Existing".to_string(),
            email: String::new(),
            config_dir: "/tmp/existing".to_string(),
            sync_dir: "~/OneDrive-A".to_string(),
            status: AccountStatus::Authenticated,
        };

        let error = create_account(&[existing], "Existing", "", "~/OneDrive-B")
            .expect_err("duplicate name should be rejected");

        assert_eq!(error, AccountStoreError::DuplicateAccountName);
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

    // The next three scenarios each require a writable XDG_CONFIG_HOME pointing
    // at a private temp dir. Because XDG_CONFIG_HOME is a process-global env
    // var and Rust runs library tests in parallel, they are fused into a single
    // serial test to avoid cross-test env mutation.
    #[test]
    fn store_methods_mutate_and_persist() {
        // Capture and restore XDG_CONFIG_HOME so this test never leaks an env
        // mutation to other tests running in parallel in the same process.
        let original = std::env::var("XDG_CONFIG_HOME").ok();
        let dir = std::env::temp_dir().join(format!(
            "onesync-store-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the original value is restored below; scenarios run
        // sequentially within this test.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };

        let sync_a = dir.join("sync-a").to_string_lossy().to_string();
        // update_status mutates and persists.
        let mut store = AccountStore::default();
        store.add("X", "", &sync_a).unwrap();
        let id = store.accounts[0].id.clone();
        store
            .update_status(&id, AccountStatus::Authenticated)
            .unwrap();
        let reloaded = load_store().unwrap();
        assert_eq!(reloaded.accounts[0].status, AccountStatus::Authenticated);

        // update_identity replaces a default name and carries email.
        let sync_b = dir.join("sync-b").to_string_lossy().to_string();
        let mut store = AccountStore::default();
        store.add("OneDrive", "", &sync_b).unwrap();
        let id = store.accounts[0].id.clone();
        let changed = store
            .update_identity(&id, Some("Alice"), Some("alice@x.com"))
            .unwrap();
        assert!(changed);
        assert_eq!(store.accounts[0].name, "Alice");
        assert_eq!(store.accounts[0].email, "alice@x.com");

        // remove drops the account.
        let sync_c = dir.join("sync-c").to_string_lossy().to_string();
        let mut store = AccountStore::default();
        store.add("Y", "", &sync_c).unwrap();
        let id = store.accounts[0].id.clone();
        store.remove(&id).unwrap();
        assert!(store.accounts.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
        // SAFETY: restoring a process-global env var; no other test should
        // depend on the value this test set.
        match &original {
            Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
    }
}
