use std::fmt;
use std::io;
use std::path::Path;

use crate::profile::{Account, ConfigEdit, OneDriveConfig, write_sync_list};

#[derive(Debug)]
pub enum ProfileEditError {
    ConfigRead(io::Error),
    ConfigWrite(io::Error),
    SyncListWrite(io::Error),
}

impl fmt::Display for ProfileEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileEditError::ConfigRead(error) => {
                write!(f, "读取 Profile 配置失败: {error}")
            }
            ProfileEditError::ConfigWrite(error) => {
                write!(f, "保存 Profile 配置失败: {error}")
            }
            ProfileEditError::SyncListWrite(error) => {
                write!(f, "保存选择性同步列表失败: {error}")
            }
        }
    }
}

impl std::error::Error for ProfileEditError {}

pub struct ProfileEditOutcome {
    pub sync_dir_changed: bool,
    pub needs_resync: bool,
}

pub fn save_profile_edit(
    account: &Account,
    original: &ConfigEdit,
    next: &ConfigEdit,
) -> Result<ProfileEditOutcome, ProfileEditError> {
    let config_path = Path::new(&account.config_dir).join("config");
    let mut config = OneDriveConfig::read(&config_path).map_err(ProfileEditError::ConfigRead)?;
    let needs_resync = original.requires_resync_from(next);
    let sync_dir_changed = original.sync_dir != next.sync_dir;
    let mut config_without_sync_list = next.clone();
    config_without_sync_list.sync_list.clear();
    config.apply_edit(&config_without_sync_list);
    config
        .write_with_backup(&config_path)
        .map_err(ProfileEditError::ConfigWrite)?;
    write_sync_list(&account.config_dir, &next.sync_list)
        .map_err(ProfileEditError::SyncListWrite)?;
    Ok(ProfileEditOutcome {
        sync_dir_changed,
        needs_resync,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::read_sync_list;
    use std::fs;

    fn make_account(dir: &std::path::Path) -> Account {
        let config_dir = dir.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config"),
            "sync_dir = \"~/OneDrive\"\nmonitor_interval = \"300\"\n",
        )
        .unwrap();
        Account {
            id: "personal".to_string(),
            name: "Personal".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: crate::profile::AccountStatus::NeedsAuth,
        }
    }

    fn read_field(config_dir: &str, key: &str) -> String {
        let content = fs::read_to_string(format!("{config_dir}/config")).unwrap();
        for line in content.lines() {
            if let Some((k, v)) = line.trim().split_once('=')
                && k.trim() == key
            {
                return crate::utils::unquote(v.trim()).to_string();
            }
        }
        String::new()
    }

    #[test]
    fn sync_dir_change_sets_flags() {
        let dir = std::env::temp_dir().join(format!(
            "onesync-edit-syncdir-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let account = make_account(&dir);
        let original = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            ..ConfigEdit::default()
        };
        let next = ConfigEdit {
            sync_dir: "~/Another".to_string(),
            ..ConfigEdit::default()
        };
        let outcome = save_profile_edit(&account, &original, &next).unwrap();
        assert!(outcome.sync_dir_changed);
        assert!(outcome.needs_resync);
        assert_eq!(read_field(&account.config_dir, "sync_dir"), "~/Another");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn non_resync_field_change_keeps_resync_false() {
        let dir = std::env::temp_dir().join(format!(
            "onesync-edit-nonresync-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let account = make_account(&dir);
        let original = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            monitor_interval: "300".to_string(),
            ..ConfigEdit::default()
        };
        let next = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            monitor_interval: "60".to_string(),
            ..ConfigEdit::default()
        };
        let outcome = save_profile_edit(&account, &original, &next).unwrap();
        assert!(!outcome.sync_dir_changed);
        assert!(!outcome.needs_resync);
        assert_eq!(read_field(&account.config_dir, "monitor_interval"), "60");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn skip_file_change_triggers_resync() {
        let dir = std::env::temp_dir().join(format!(
            "onesync-edit-skipfile-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let account = make_account(&dir);
        let original = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            skip_file: vec!["*.tmp".to_string()],
            ..ConfigEdit::default()
        };
        let next = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            skip_file: vec!["*.bak".to_string()],
            ..ConfigEdit::default()
        };
        let outcome = save_profile_edit(&account, &original, &next).unwrap();
        assert!(!outcome.sync_dir_changed);
        assert!(outcome.needs_resync);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sync_list_is_written_and_resync_flagged() {
        let dir = std::env::temp_dir().join(format!(
            "onesync-edit-synclist-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let account = make_account(&dir);
        let original = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            sync_list: "Folder1/*\n".to_string(),
            ..ConfigEdit::default()
        };
        let next = ConfigEdit {
            sync_dir: "~/OneDrive".to_string(),
            sync_list: "Folder2/*\n".to_string(),
            ..ConfigEdit::default()
        };
        let outcome = save_profile_edit(&account, &original, &next).unwrap();
        assert!(!outcome.sync_dir_changed);
        assert!(outcome.needs_resync);
        assert_eq!(read_sync_list(&account.config_dir).unwrap(), "Folder2/*\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_config_file_returns_config_read_error() {
        let dir = std::env::temp_dir().join(format!(
            "onesync-edit-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let config_dir = dir.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        let account = Account {
            id: "personal".to_string(),
            name: "Personal".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: crate::profile::AccountStatus::NeedsAuth,
        };
        let result = save_profile_edit(&account, &ConfigEdit::default(), &ConfigEdit::default());
        assert!(matches!(result, Err(ProfileEditError::ConfigRead(_))));
        fs::remove_dir_all(&dir).unwrap();
    }
}
