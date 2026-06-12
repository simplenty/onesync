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
    let normalized = path.trim_start_matches("./").trim_matches('/');
    let Some((parent, _)) = normalized.rsplit_once('/') else {
        return;
    };
    if !parent.is_empty() {
        command.arg("--single-directory").arg(parent);
    }
}
