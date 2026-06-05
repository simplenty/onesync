use crate::event::BackendEvent;
use crate::profile::Account;
use serde::Deserialize;
use std::{fs, io, path::Path, sync::mpsc, thread, time::Duration};

const DEFAULT_APPLICATION_ID: &str = "d50ca740-c83f-4d1b-b616-12c519384f0c";
const GRAPH_TOKEN_SCOPE: &str = "https://graph.microsoft.com/Files.ReadWrite offline_access";
const NATIVE_CLIENT_REDIRECT_URI: &str =
    "https://login.microsoftonline.com/common/oauth2/nativeclient";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct DriveResponse {
    owner: Option<DriveOwner>,
}

#[derive(Debug, Deserialize)]
struct DriveOwner {
    user: Option<DriveOwnerUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveOwnerUser {
    display_name: Option<String>,
    email: Option<String>,
}

pub fn start_account_identity_lookup(account: Account, sender: mpsc::Sender<BackendEvent>) {
    thread::spawn(move || {
        let result = fetch_account_identity(&account);
        let (display_name, email, message) = match result {
            Ok(identity) => (identity.display_name, identity.email, None),
            Err(error) => (
                None,
                None,
                Some(format!("无法读取 Microsoft 账号信息: {error}")),
            ),
        };
        let _ = sender.send(BackendEvent::AccountIdentityFound {
            account_id: account.id,
            display_name,
            email,
            message,
        });
    });
}

fn fetch_account_identity(account: &Account) -> io::Result<AccountIdentity> {
    let access_token = graph_access_token(account)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(io::Error::other)?;

    let drive = client
        .get("https://graph.microsoft.com/v1.0/me/drive?$select=owner")
        .bearer_auth(access_token)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(io::Error::other)?
        .json::<DriveResponse>()
        .map_err(io::Error::other)?;

    let user = drive.owner.and_then(|owner| owner.user);
    Ok(AccountIdentity {
        display_name: user
            .as_ref()
            .and_then(|user| non_empty(user.display_name.as_deref())),
        email: user.and_then(|user| non_empty(user.email.as_deref())),
    })
}

pub(crate) fn graph_access_token(account: &Account) -> io::Result<String> {
    let refresh_token = fs::read_to_string(Path::new(&account.config_dir).join("refresh_token"))?;
    let refresh_token = refresh_token.trim();
    if refresh_token.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "refresh_token is empty",
        ));
    }

    let application_id =
        application_id(&account.config_dir).unwrap_or_else(|| DEFAULT_APPLICATION_ID.to_string());
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(io::Error::other)?;

    client
        .post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
        .form(&[
            ("client_id", application_id.as_str()),
            ("redirect_uri", NATIVE_CLIENT_REDIRECT_URI),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", GRAPH_TOKEN_SCOPE),
        ])
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(io::Error::other)?
        .json::<TokenResponse>()
        .map(|token| token.access_token)
        .map_err(io::Error::other)
}

fn application_id(config_dir: &str) -> Option<String> {
    let config = fs::read_to_string(Path::new(config_dir).join("config")).ok()?;
    config.lines().find_map(|line| {
        let (key, value) = line.trim().split_once('=')?;
        (key.trim() == "application_id")
            .then(|| unquote(value))
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_blank_identity_fields() {
        assert_eq!(non_empty(Some("  John Li  ")), Some("John Li".to_string()));
        assert_eq!(non_empty(Some("  ")), None);
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn parses_quoted_config_values() {
        assert_eq!(unquote(r#" "application-id" "#), "application-id");
        assert_eq!(unquote(" 'application-id' "), "application-id");
    }
}
