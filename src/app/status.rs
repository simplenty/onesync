use crate::account::{Account, AccountStatus};

pub(in crate::app) fn status_title(status: &AccountStatus) -> &'static str {
    match status {
        AccountStatus::NeedsAuth => "需要认证",
        AccountStatus::Authenticating => "认证中",
        AccountStatus::Authenticated => "已认证",
        AccountStatus::Syncing => "同步中",
        AccountStatus::Monitoring => "持续同步中",
        AccountStatus::Error(_) => "需要处理",
    }
}

pub(in crate::app) fn status_label(status: &AccountStatus) -> &str {
    match status {
        AccountStatus::NeedsAuth => "未认证",
        AccountStatus::Authenticating => "认证中",
        AccountStatus::Authenticated => "已认证",
        AccountStatus::Syncing => "同步中",
        AccountStatus::Monitoring => "持续同步中",
        AccountStatus::Error(message) => message.as_str(),
    }
}

pub(in crate::app) fn status_detail(account: &Account) -> String {
    match &account.status {
        AccountStatus::NeedsAuth => format!("配置目录: {}", account.config_dir),
        AccountStatus::Authenticating => "打开认证链接，登录后粘贴 redirect URI".to_string(),
        AccountStatus::Authenticated => format!("同步目录: {}", account.sync_dir),
        AccountStatus::Syncing => "onedrive CLI 正在执行一次同步".to_string(),
        AccountStatus::Monitoring => "onedrive CLI 正在持续监听本地和远端变化".to_string(),
        AccountStatus::Error(message) => format!("最近错误: {message}"),
    }
}

pub(in crate::app) fn account_label(account: &Account) -> String {
    if account.email.trim().is_empty() {
        account.id.clone()
    } else {
        account.email.clone()
    }
}
