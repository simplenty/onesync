use super::{
    auth, confirm,
    render::{refresh_content, show_toast},
    state::{ActiveOperation, AppState},
    update_account_status,
};
use crate::{
    account::{Account, AccountStatus},
    onedrive::{BackendEvent, MonitorHandle, SyncHandle, stop_handle, stop_monitor_handle},
};
use adw::prelude::*;
use gtk::glib;
use std::{collections::VecDeque, rc::Rc, time::Duration};

use super::status::status_label;

pub(super) fn install_backend_event_pump(state: Rc<AppState>) {
    glib::timeout_add_local(Duration::from_millis(250), move || {
        drain_backend_events(&state);
        glib::ControlFlow::Continue
    });
}

fn drain_backend_events(state: &Rc<AppState>) {
    let mut events = VecDeque::new();
    {
        let receiver = state.receiver.borrow();
        while let Ok(event) = receiver.try_recv() {
            events.push_back(event);
        }
    }

    while let Some(event) = events.pop_front() {
        match event {
            BackendEvent::ClientChecked(check) => {
                let message = check.message();
                state.client_check.replace(check);
                refresh_content(state);
                show_toast(state, &message);
            }
            BackendEvent::AuthUrl { account_id, url } => {
                if let Some(panel) = state.auth_panel.borrow().as_ref()
                    && panel.account_id == account_id
                {
                    panel.auth_url_entry.set_text(&url);
                    panel
                        .status_label
                        .set_label("认证链接已生成，请复制到浏览器登录");
                }
                if state
                    .selected_account_id()
                    .is_some_and(|id| id == account_id)
                {
                    show_toast(state, "认证链接已生成");
                }
            }
            BackendEvent::AuthFinished {
                account_id,
                success,
                message,
            } => {
                finish_active_operation(state, &account_id);
                let status = if success {
                    AccountStatus::Authenticated
                } else {
                    AccountStatus::Error(message.unwrap_or_else(|| "认证失败".to_string()))
                };
                update_account_status(state, &account_id, status);
                state.transfers.clear();
                if let Some(panel) = state.auth_panel.borrow().as_ref()
                    && panel.account_id == account_id
                {
                    panel.status_label.set_label(if success {
                        "认证完成，可以关闭窗口"
                    } else {
                        "认证失败，请检查输出后重试"
                    });
                }
                if success {
                    show_toast(state, "认证完成");
                } else if let Some(account) = state.selected_account() {
                    show_toast(state, status_label(&account.status));
                } else {
                    show_toast(state, "认证失败");
                }
            }
            BackendEvent::SyncFinished {
                account_id,
                success,
                requested_stop,
                auth_required,
                message,
                requires_confirmation,
            } => {
                state.syncs.borrow_mut().remove(&account_id);
                finish_active_operation(state, &account_id);
                if auth_required {
                    handle_auth_required(Rc::clone(state), &account_id, message);
                    continue;
                }
                let status = if success || requested_stop {
                    AccountStatus::Authenticated
                } else {
                    AccountStatus::Error(message.unwrap_or_else(|| "同步失败".to_string()))
                };
                update_account_status(state, &account_id, status);
                if let Some(kind) = requires_confirmation {
                    confirm::show_warning_window(state, "需要确认", kind.user_message());
                } else if requested_stop {
                    show_toast(state, "同步已停止");
                } else if success {
                    show_toast(state, "同步完成");
                } else if let Some(account) = state.selected_account() {
                    show_toast(state, status_label(&account.status));
                }
            }
            BackendEvent::LogoutFinished {
                account_id,
                success,
                message,
            } => {
                finish_active_operation(state, &account_id);
                let status = if success {
                    AccountStatus::NeedsAuth
                } else {
                    AccountStatus::Error(message.unwrap_or_else(|| "退出登录失败".to_string()))
                };
                update_account_status(state, &account_id, status);
                show_toast(
                    state,
                    if success {
                        "已退出登录"
                    } else {
                        "退出登录失败"
                    },
                );
            }
            BackendEvent::TransferEvent { account_id, file } => {
                if state
                    .selected_account_id()
                    .is_some_and(|id| id == account_id)
                {
                    state.transfers.upsert(file);
                }
            }
            BackendEvent::MonitorStopped {
                account_id,
                success,
                requested_stop,
                auth_required,
                message,
                requires_confirmation,
            } => {
                state.monitors.borrow_mut().remove(&account_id);
                finish_active_operation(state, &account_id);
                if auth_required {
                    handle_auth_required(Rc::clone(state), &account_id, message);
                    continue;
                }
                let status = if success || requested_stop {
                    AccountStatus::Authenticated
                } else {
                    AccountStatus::Error(message.unwrap_or_else(|| "持续同步停止".to_string()))
                };
                update_account_status(state, &account_id, status);
                if let Some(kind) = requires_confirmation {
                    confirm::show_warning_window(state, "需要确认", kind.user_message());
                } else if requested_stop {
                    show_toast(state, "持续同步已停止");
                } else if success {
                    show_toast(state, "持续同步已结束");
                } else {
                    show_toast(state, "持续同步异常停止");
                }
            }
        }
    }
}

pub(super) fn stop_monitor(state: &AppState, account_id: &str) {
    let Some(handle) = state.monitors.borrow().get(account_id).cloned() else {
        show_toast(state, "持续同步未运行");
        return;
    };

    state
        .active_operations
        .borrow_mut()
        .insert(account_id.to_string(), ActiveOperation::StoppingMonitor);
    refresh_content(state);
    match stop_monitor_handle(&handle) {
        Ok(()) => show_toast(state, "正在停止持续同步"),
        Err(error) => {
            finish_active_operation(state, account_id);
            show_toast(state, &format!("停止持续同步失败: {error}"));
        }
    }
}

pub(super) fn stop_sync(state: &AppState, account_id: &str) {
    let Some(handle) = state.syncs.borrow().get(account_id).cloned() else {
        show_toast(state, "一次同步未运行");
        return;
    };

    state
        .active_operations
        .borrow_mut()
        .insert(account_id.to_string(), ActiveOperation::StoppingSync);
    refresh_content(state);
    match stop_handle(&handle) {
        Ok(()) => show_toast(state, "正在停止同步"),
        Err(error) => {
            finish_active_operation(state, account_id);
            show_toast(state, &format!("停止同步失败: {error}"));
        }
    }
}

pub(super) fn stop_all_monitors(state: &AppState) {
    let sync_handles: Vec<SyncHandle> = state.syncs.borrow().values().cloned().collect();
    for handle in sync_handles {
        let _ = stop_handle(&handle);
    }
    let handles: Vec<MonitorHandle> = state.monitors.borrow().values().cloned().collect();
    for handle in handles {
        let _ = stop_monitor_handle(&handle);
    }
}

pub(super) fn is_monitor_running(state: &AppState, account_id: &str) -> bool {
    state.monitors.borrow().contains_key(account_id)
}

pub(super) fn is_sync_running(state: &AppState, account_id: &str) -> bool {
    state.syncs.borrow().contains_key(account_id)
}

pub(super) fn has_active_operation(state: &AppState, account_id: &str) -> bool {
    state.active_operations.borrow().contains_key(account_id)
}

pub(super) fn active_operation(state: &AppState, account_id: &str) -> Option<ActiveOperation> {
    state.active_operations.borrow().get(account_id).copied()
}

pub(super) fn can_mutate_profile(state: &AppState, account: &Account) -> bool {
    !matches!(
        account.status,
        AccountStatus::Authenticating | AccountStatus::Syncing | AccountStatus::Monitoring
    ) && !has_active_operation(state, &account.id)
        && !is_sync_running(state, &account.id)
        && !is_monitor_running(state, &account.id)
}

pub(super) fn begin_active_operation(
    state: &AppState,
    account_id: &str,
    operation: ActiveOperation,
) -> bool {
    if state.active_operations.borrow().contains_key(account_id) {
        show_active_operation_toast(state, account_id);
        return false;
    }
    state
        .active_operations
        .borrow_mut()
        .insert(account_id.to_string(), operation);
    refresh_content(state);
    true
}

pub(super) fn finish_active_operation(state: &AppState, account_id: &str) {
    state.active_operations.borrow_mut().remove(account_id);
    refresh_content(state);
}

pub(super) fn show_active_operation_toast(state: &AppState, account_id: &str) {
    let operation = state.active_operations.borrow().get(account_id).copied();
    let message = operation.map_or("该 profile 正在运行操作".to_string(), |operation| {
        format!("该 profile 正在执行{}", operation.label())
    });
    show_toast(state, &message);
}

fn handle_auth_required(state: Rc<AppState>, account_id: &str, message: Option<String>) {
    update_account_status(&state, account_id, AccountStatus::NeedsAuth);
    show_toast(
        &state,
        message.as_deref().unwrap_or("认证已失效，请重新完成登录"),
    );
    let account = state
        .accounts
        .borrow()
        .iter()
        .find(|account| account.id == account_id)
        .cloned();
    if let Some(account) = account {
        auth::show_auth_dialog(state, account);
    }
}

pub(super) fn ensure_client_ready(state: &AppState) -> bool {
    let check = state.client_check.borrow();
    if check.is_ready() {
        true
    } else {
        show_toast(state, &check.message());
        false
    }
}
