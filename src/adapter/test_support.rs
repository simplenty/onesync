#![cfg(test)]

#[cfg(unix)]
use crate::profile::Account;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
pub fn fake_onedrive_binary(output: &str, exit_code: i32) -> (PathBuf, PathBuf) {
    use std::{
        env,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    let root = env::temp_dir().join(format!(
        "onesync-fake-onedrive-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let args_file = root.join("args");
    let binary = root.join("fake-onedrive");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\nexit {}\n",
            args_file.display(),
            output.replace('\'', "'\\''"),
            exit_code
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&binary, permissions).unwrap();
    (binary, args_file)
}

#[cfg(unix)]
pub fn temp_account(prefix: &str) -> Account {
    use crate::profile::AccountStatus;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    let root = env::temp_dir().join(format!(
        "onesync-account-{}-{}",
        prefix,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let config_dir = root.join("profile");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();

    Account {
        id: "test-account".to_string(),
        name: "Test Account".to_string(),
        email: String::new(),
        config_dir: config_dir.to_string_lossy().to_string(),
        sync_dir: "~/OneDrive".to_string(),
        status: AccountStatus::Authenticated,
    }
}

#[cfg(unix)]
pub fn sync_test_fixture(
    label: &str,
    script_body: &str,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    use std::{
        env,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    let root = env::temp_dir().join(format!(
        "onesync-{label}-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let config_dir = root.join("profile");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();
    let binary = root.join("fake-onedrive");
    std::fs::write(
        &binary,
        format!("#!/bin/sh\ncd \"$(dirname \"$0\")\"\n{script_body}"),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&binary, permissions).unwrap();
    (binary, config_dir, root)
}
