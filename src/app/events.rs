use super::{
    auth, confirm,
    render::{rebuild_profile_list, refresh_content, show_toast},
    state::{ActiveOperation, AppState},
    status_label, update_account_status,
};
use crate::{
    account::{Account, AccountStatus, save_accounts},
    onedrive::{
        BackendEvent, ConfirmationKind, MonitorHandle, SyncHandle, start_account_identity_lookup,
        stop_handle, stop_monitor_handle,
    },
};
use adw::prelude::*;
use gtk::glib;
use std::{collections::VecDeque, rc::Rc, time::Duration};

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
                    panel.close_blocked.set(false);
                    panel.close_button.set_sensitive(true);
                    panel.copy_auth_url_button.set_sensitive(false);
                    panel.finish_button.set_sensitive(false);
                    panel.status_label.set_label(if success {
                        "认证完成，可以关闭窗口"
                    } else {
                        "认证失败，请检查输出后重试"
                    });
                }
                if success {
                    show_toast(state, "认证完成");
                    if let Some(account) = account_by_id(state, &account_id) {
                        start_account_identity_lookup(account, state.sender.clone());
                    }
                } else if let Some(account) = state.selected_account() {
                    show_toast(state, status_label(&account.status));
                } else {
                    show_toast(state, "认证失败");
                }
            }
            BackendEvent::AccountIdentityFound {
                account_id,
                display_name,
                email,
                message,
            } => {
                if update_account_identity(
                    state,
                    &account_id,
                    display_name.as_deref(),
                    email.as_deref(),
                ) {
                    rebuild_profile_list(state);
                    refresh_content(state);
                }
                if let Some(message) = message {
                    show_toast(state, &message);
                } else if let Some(email) = email {
                    show_toast(state, &format!("账号: {email}"));
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
                    handle_confirmation_required(Rc::clone(state), &account_id, kind);
                } else if requested_stop {
                    show_toast(state, "同步已停止");
                } else if success {
                    show_toast(state, "同步完成");
                } else if let Some(account) = state.selected_account() {
                    show_toast(state, status_label(&account.status));
                }
            }
            BackendEvent::TransferEvent { account_id, file } => {
                if state
                    .selected_account_id()
                    .is_some_and(|id| id == account_id)
                {
                    state.transfers.upsert(file);
                }
            }
            BackendEvent::PreviewEvent { account_id, change } => {
                if state
                    .selected_account_id()
                    .is_some_and(|id| id == account_id)
                {
                    state
                        .previews
                        .borrow_mut()
                        .entry(account_id.clone())
                        .or_default()
                        .insert(change.id.clone(), change.clone());
                    state.transfers.upsert_preview(account_id, change);
                }
            }
            BackendEvent::PreviewFinished {
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
                if let Some(kind) = requires_confirmation {
                    handle_confirmation_required(Rc::clone(state), &account_id, kind);
                } else if requested_stop {
                    show_toast(state, "预览已停止");
                } else if success {
                    show_toast(state, "预览完成");
                } else {
                    show_toast(state, message.as_deref().unwrap_or("预览失败"));
                }
            }
            BackendEvent::PreviewApplyFinished {
                account_id,
                change_id,
                success,
                message,
            } => {
                state
                    .applying_preview_changes
                    .borrow_mut()
                    .remove(&(account_id.clone(), change_id.clone()));
                if success {
                    show_toast(state, "云端操作已完成，正在更新同步状态");
                } else {
                    state.transfers.mark_preview_failed(
                        &account_id,
                        &change_id,
                        message.as_deref().unwrap_or("应用失败"),
                    );
                    show_toast(state, message.as_deref().unwrap_or("应用预览变更失败"));
                }
            }
            BackendEvent::PreviewApplyProgress {
                account_id,
                change_id,
                progress,
            } => {
                state
                    .transfers
                    .mark_preview_progress(&account_id, &change_id, progress);
            }
            BackendEvent::PreviewReconcileStarted {
                account_id,
                change_id,
                scope: _,
            } => {
                state
                    .transfers
                    .mark_preview_reconciling(&account_id, &change_id);
            }
            BackendEvent::PreviewReconcileFinished {
                account_id,
                change_id,
                success,
                message,
            } => {
                if success {
                    if let Some(previews) = state.previews.borrow_mut().get_mut(&account_id) {
                        previews.remove(&change_id);
                    }
                    state
                        .transfers
                        .mark_preview_applied(&account_id, &change_id);
                    show_toast(state, "已应用并更新同步状态");
                } else {
                    state.transfers.mark_preview_reconcile_failed(
                        &account_id,
                        &change_id,
                        message.as_deref().unwrap_or("同步状态更新失败"),
                    );
                    show_toast(state, message.as_deref().unwrap_or("同步状态更新失败"));
                }
            }
            BackendEvent::ConfirmationRequired { account_id, kind } => {
                if matches!(kind, ConfirmationKind::BigDelete) {
                    show_toast(state, kind.user_message());
                } else if state
                    .selected_account_id()
                    .is_some_and(|id| id == account_id)
                {
                    confirm::show_warning_window(state, "需要确认", kind.user_message());
                } else {
                    show_toast(state, kind.user_message());
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
                    handle_confirmation_required(Rc::clone(state), &account_id, kind);
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

pub(super) fn stop_sync(
    state: &AppState,
    account_id: &str,
    operation: ActiveOperation,
    message: &str,
) {
    let Some(handle) = state.syncs.borrow().get(account_id).cloned() else {
        show_toast(state, "同步未运行");
        return;
    };

    state
        .active_operations
        .borrow_mut()
        .insert(account_id.to_string(), operation);
    refresh_content(state);
    if let Err(error) = stop_handle(&handle) {
        finish_active_operation(state, account_id);
        show_toast(state, &format!("{message}失败: {error}"));
    }
}

fn handle_confirmation_required(state: Rc<AppState>, account_id: &str, kind: ConfirmationKind) {
    if matches!(kind, ConfirmationKind::BigDelete)
        && let Some(account) = account_by_id(&state, account_id)
    {
        confirm::show_big_delete_confirmation(state, account);
        return;
    }

    confirm::show_warning_window(&state, "需要确认", kind.user_message());
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
    if let Err(error) = stop_monitor_handle(&handle) {
        finish_active_operation(state, account_id);
        show_toast(state, &format!("停止持续同步失败: {error}"));
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
    let message = operation.map_or("该账户正在运行操作".to_string(), |operation| {
        format!("该账户正在执行{}", operation.label())
    });
    show_toast(state, &message);
}

fn account_by_id(state: &AppState, account_id: &str) -> Option<Account> {
    state
        .accounts
        .borrow()
        .iter()
        .find(|account| account.id == account_id)
        .cloned()
}

fn update_account_identity(
    state: &AppState,
    account_id: &str,
    display_name: Option<&str>,
    email: Option<&str>,
) -> bool {
    let mut changed = false;
    {
        let mut accounts = state.accounts.borrow_mut();
        let Some(account) = accounts.iter_mut().find(|account| account.id == account_id) else {
            return false;
        };
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

    if changed && let Err(error) = save_accounts(&state.accounts.borrow()) {
        show_toast(state, &format!("保存账号信息失败: {error}"));
    }

    changed
}

fn should_replace_profile_name(current_name: &str, current_email: &str) -> bool {
    let name = current_name.trim();
    name.is_empty() || name == "OneDrive" || name.starts_with("OneDrive ") || name == current_email
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
