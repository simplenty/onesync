use crate::profile::Account;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OneDriveCommandKind {
    Sync { force: bool, resync: bool },
    Preview,
    Monitor,
    DisplaySyncStatus,
    ReconcileSync,
}

pub(super) fn build_command(
    binary: String,
    account: &Account,
    kind: OneDriveCommandKind,
) -> Command {
    let mut command = Command::new(binary);
    command.arg("--confdir").arg(&account.config_dir);
    match kind {
        OneDriveCommandKind::Sync { force, resync } => {
            command.arg("--sync").arg("--verbose");
            if force {
                command.arg("--force");
            }
            if resync {
                command.arg("--resync").arg("--resync-auth");
            }
        }
        OneDriveCommandKind::Preview => {
            command
                .arg("--sync")
                .arg("--verbose")
                .arg("--local-first")
                .arg("--dry-run");
        }
        OneDriveCommandKind::Monitor => {
            command.arg("--monitor").arg("--verbose");
        }
        OneDriveCommandKind::DisplaySyncStatus => {
            command.arg("--display-sync-status");
        }
        OneDriveCommandKind::ReconcileSync => {
            command.arg("--sync").arg("--verbose");
        }
    }
    command
}

pub(super) fn add_single_directory_scope(command: &mut Command, path: &str) {
    if let Some(parent) = crate::utils::sync_path(path).parent {
        command.arg("--single-directory").arg(parent);
    }
}
