use crate::event::{BackendError, ConfirmationKind, Version};

pub(super) fn parse_version(output: &str) -> Option<Version> {
    let (_, version) = output.split_once("onedrive")?;
    let mut parts = version
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .find(|part| part.chars().any(|character| character.is_ascii_digit()))?
        .split('.');
    Some(Version {
        major: parts.next()?.parse().ok()?,
        minor: parts.next().unwrap_or("0").parse().ok()?,
        patch: parts.next().unwrap_or("0").parse().ok()?,
    })
}

pub(super) fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut combined = String::from_utf8_lossy(stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(stderr));
    combined
}

pub(super) fn classify_onedrive_error(output: &str) -> BackendError {
    let lower = output.to_ascii_lowercase();
    if is_auth_required(output) {
        BackendError::AuthExpired
    } else if lower.contains("could not resolve")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("timeout")
    {
        BackendError::Network
    } else if lower.contains("unknown key") || lower.contains("unknown config") {
        BackendError::UnsupportedConfig
    } else if lower.contains("failed") && (lower.contains("upload") || lower.contains("download")) {
        BackendError::PartialTransfer
    } else if lower.contains("segmentation fault") || lower.contains("core dumped") {
        BackendError::CliCrashed
    } else if lower.contains("auth") || lower.contains("unauthorized") {
        BackendError::AuthFailed
    } else {
        BackendError::CliOutput(
            output
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim()
                .to_string(),
        )
    }
}

pub(super) fn is_auth_required(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("login required")
        || lower.contains("authorise this application")
        || lower.contains("authorize this application")
        || lower.contains("re-authorise")
        || lower.contains("re-authorize")
        || lower.contains("fresh auth token")
        || lower.contains("provided grant has expired")
        || lower.contains("refresh_token is invalid")
        || lower.contains("refresh token is invalid")
        || lower.contains("reauthenticate")
        || lower.contains("reauth")
}

pub(super) fn parse_confirmation(output: &str) -> Option<ConfirmationKind> {
    let lower = output.to_ascii_lowercase();
    if is_resync_confirmation(&lower) {
        Some(ConfirmationKind::ResyncRequired)
    } else if is_big_delete_confirmation(&lower) {
        Some(ConfirmationKind::BigDelete)
    } else if is_download_only_cleanup_confirmation(&lower) {
        Some(ConfirmationKind::DownloadOnlyCleanup)
    } else if is_upload_only_no_remote_delete_confirmation(&lower) {
        Some(ConfirmationKind::UploadOnlyNoRemoteDelete)
    } else {
        None
    }
}

fn is_resync_confirmation(lower: &str) -> bool {
    lower.lines().any(|line| {
        let line = line.trim();
        line.contains("--resync")
            && (line.contains("required")
                || line.contains("must be used")
                || line.contains("use --resync")
                || line.contains("wish to proceed")
                || line.contains("asked the client to perform"))
    })
}

fn is_big_delete_confirmation(lower: &str) -> bool {
    lower.lines().any(|line| {
        let line = line.trim();
        line.contains("big delete detected")
            || (line.contains("big delete")
                && line.contains("detected")
                && (line.contains("--force") || line.contains("force")))
            || line.contains("attempt to remove a large volume of data from onedrive")
            || (line.contains("to delete a large volume of data")
                && line.contains("--force")
                && line.contains("classify_as_big_delete"))
    })
}

fn is_download_only_cleanup_confirmation(lower: &str) -> bool {
    lower.lines().any(|line| {
        let line = line.trim();
        line.contains("download-only")
            && (line.contains("cleanup") || line.contains("clean up"))
            && (line.contains("warning") || line.contains("risk") || line.contains("cannot"))
            || (line.contains("download-only")
                && line.contains("remove local data")
                && (line.contains("cleanup") || line.contains("clean up")))
    })
}

fn is_upload_only_no_remote_delete_confirmation(lower: &str) -> bool {
    lower.lines().any(|line| {
        let line = line.trim();
        line.contains("upload-only")
            && line.contains("no-remote-delete")
            && (line.contains("cannot")
                || line.contains("invalid")
                || line.contains("not permitted")
                || line.contains("incompatible"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_from_cli_output() {
        assert_eq!(
            parse_version("onedrive v2.5.4-1+np1").unwrap(),
            Version {
                major: 2,
                minor: 5,
                patch: 4
            }
        );
    }

    #[test]
    fn maps_known_error_output_to_actionable_messages() {
        assert_eq!(
            classify_onedrive_error("ERROR: refresh_token is invalid"),
            BackendError::AuthExpired
        );
        assert_eq!(
            classify_onedrive_error("curl timeout while connecting"),
            BackendError::Network
        );
        assert_eq!(
            classify_onedrive_error("unknown config key: verbose"),
            BackendError::UnsupportedConfig
        );
    }

    #[test]
    fn classifies_unrecognized_output_as_cli_output() {
        assert_eq!(
            classify_onedrive_error("some random stderr line\nERROR: something else"),
            BackendError::CliOutput("ERROR: something else".to_string())
        );
        assert_eq!(
            classify_onedrive_error(""),
            BackendError::CliOutput(String::new())
        );
    }

    #[test]
    fn detects_login_required_output() {
        assert!(is_auth_required("ERROR: Login required"));
        assert!(is_auth_required(
            "To authorise this application open the URL"
        ));
        assert!(is_auth_required("ERROR: refresh_token is invalid"));
        assert!(is_auth_required(
            "ERROR: You will need to issue a --reauth and re-authorise this client to obtain a fresh auth token."
        ));
        assert!(is_auth_required(
            "AADSTS50173: The provided grant has expired due to it being revoked"
        ));
    }

    #[test]
    fn detects_confirmation_required_states() {
        assert!(matches!(
            parse_confirmation("--resync is required to continue"),
            Some(ConfirmationKind::ResyncRequired)
        ));
        assert!(matches!(
            parse_confirmation("Are you sure you wish to proceed with --resync? [Y/N]"),
            Some(ConfirmationKind::ResyncRequired)
        ));
        assert!(matches!(
            parse_confirmation(
                "WARNING: You have asked the client to perform a --resync operation."
            ),
            Some(ConfirmationKind::ResyncRequired)
        ));
        assert!(matches!(
            parse_confirmation("ERROR: big delete detected"),
            Some(ConfirmationKind::BigDelete)
        ));
        assert!(matches!(
            parse_confirmation("ERROR: Big Delete detected; rerun with --force to continue"),
            Some(ConfirmationKind::BigDelete)
        ));
        assert!(matches!(
            parse_confirmation(
                "ERROR: To delete a large volume of data use --force or increase the config value 'classify_as_big_delete' to a larger value"
            ),
            Some(ConfirmationKind::BigDelete)
        ));
        assert!(matches!(
            parse_confirmation("download-only cleanup warning"),
            Some(ConfirmationKind::DownloadOnlyCleanup)
        ));
        assert!(matches!(
            parse_confirmation(
                "Clean up additional local files when using --download-only. This will remove local data"
            ),
            Some(ConfirmationKind::DownloadOnlyCleanup)
        ));
        assert!(matches!(
            parse_confirmation("upload-only cannot be used with no-remote-delete"),
            Some(ConfirmationKind::UploadOnlyNoRemoteDelete)
        ));
    }

    #[test]
    fn avoids_false_big_delete_confirmation_matches() {
        assert!(
            parse_confirmation("Deleting item from Microsoft OneDrive: /Documents/old.txt")
                .is_none()
        );
        assert!(
            parse_confirmation("delete failed after retry; rerun with --force only if requested")
                .is_none()
        );
        assert!(parse_confirmation("large delete batch finished successfully").is_none());
        assert!(parse_confirmation("classify_as_big_delete = 1000").is_none());
    }

    #[test]
    fn avoids_false_option_combination_confirmation_matches() {
        assert!(
            parse_confirmation("download-only sync finished; cleanup was not requested").is_none()
        );
        assert!(
            parse_confirmation("upload-only profile has no-remote-delete documented in notes")
                .is_none()
        );
        assert!(parse_confirmation("resync metadata check finished without action").is_none());
    }
}
