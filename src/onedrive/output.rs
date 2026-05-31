use super::event::{ConfirmationKind, Version};

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

pub(super) fn parse_onedrive_error(output: &str) -> String {
    let lower = output.to_ascii_lowercase();
    if is_auth_required(output) {
        "认证已过期或缺少 refresh_token，请重新完成登录".to_string()
    } else if lower.contains("could not resolve")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("timeout")
    {
        "网络连接失败，请检查网络或代理后重试".to_string()
    } else if lower.contains("unknown key") || lower.contains("unknown config") {
        "配置文件包含 onedrive 不支持的选项，请编辑 profile 配置".to_string()
    } else if lower.contains("failed") && (lower.contains("upload") || lower.contains("download")) {
        "部分上传或下载失败，请查看传输列表和 onedrive 输出".to_string()
    } else if lower.contains("segmentation fault") || lower.contains("core dumped") {
        "onedrive CLI 崩溃，请升级 CLI 或检查该 profile 配置".to_string()
    } else if lower.contains("auth") || lower.contains("unauthorized") {
        "认证失败，请重新完成该 profile 登录".to_string()
    } else {
        output
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("onedrive 操作失败")
            .trim()
            .to_string()
    }
}

pub(super) fn is_auth_required(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("login required")
        || lower.contains("authorise this application")
        || lower.contains("authorize this application")
        || lower.contains("refresh_token is invalid")
        || lower.contains("refresh token is invalid")
        || lower.contains("reauth")
}

pub(super) fn parse_confirmation(output: &str) -> Option<ConfirmationKind> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("--resync") && lower.contains("required") {
        Some(ConfirmationKind::ResyncRequired)
    } else if lower.contains("big delete") || lower.contains("large delete") {
        Some(ConfirmationKind::BigDelete)
    } else if lower.contains("download-only") && lower.contains("cleanup") {
        Some(ConfirmationKind::DownloadOnlyCleanup)
    } else if lower.contains("upload-only") && lower.contains("no-remote-delete") {
        Some(ConfirmationKind::UploadOnlyNoRemoteDelete)
    } else {
        None
    }
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
            parse_onedrive_error("ERROR: refresh_token is invalid"),
            "认证已过期或缺少 refresh_token，请重新完成登录"
        );
        assert_eq!(
            parse_onedrive_error("curl timeout while connecting"),
            "网络连接失败，请检查网络或代理后重试"
        );
        assert_eq!(
            parse_onedrive_error("unknown config key: verbose"),
            "配置文件包含 onedrive 不支持的选项，请编辑 profile 配置"
        );
    }

    #[test]
    fn detects_login_required_output() {
        assert!(is_auth_required("ERROR: Login required"));
        assert!(is_auth_required(
            "To authorise this application open the URL"
        ));
        assert!(is_auth_required("ERROR: refresh_token is invalid"));
    }

    #[test]
    fn detects_confirmation_required_states() {
        assert!(matches!(
            parse_confirmation("--resync is required to continue"),
            Some(ConfirmationKind::ResyncRequired)
        ));
        assert!(matches!(
            parse_confirmation("ERROR: big delete detected"),
            Some(ConfirmationKind::BigDelete)
        ));
        assert!(matches!(
            parse_confirmation("download-only cleanup warning"),
            Some(ConfirmationKind::DownloadOnlyCleanup)
        ));
        assert!(matches!(
            parse_confirmation("upload-only cannot be used with no-remote-delete"),
            Some(ConfirmationKind::UploadOnlyNoRemoteDelete)
        ));
    }
}
