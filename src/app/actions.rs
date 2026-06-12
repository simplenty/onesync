use super::{
    dropdown_index_from_sync_mode,
    events::{
        begin_operation, ensure_client_ready, finish_operation, is_monitor_running,
        is_sync_running, stop_monitor, stop_sync,
    },
    onedrive_command,
    render::{refresh_content, show_toast},
    state::AppState,
    update_account_status,
};
use crate::{
    adapter::{
        graph::start_apply_preview_change,
        onedrive::{start_forced_sync, start_monitor, start_preview, start_resync, start_sync},
    },
    operation::OperationKind,
    profile::{Account, AccountStatus, SyncMode, is_authenticated, load_profile_sync_mode},
};
use adw::prelude::*;
use gtk::gio;
use std::rc::Rc;

pub(in crate::app) fn start_one_time_sync_for_account(state: Rc<AppState>, account: Account) {
    start_manual_one_time_sync_for_account(state, account);
}

pub(in crate::app) fn load_sync_mode_for_selected_profile(state: &AppState) {
    let mode = state
        .selected_account_id()
        .map(|account_id| {
            load_profile_sync_mode(&account_id).unwrap_or_else(|error| {
                show_toast(state, &format!("读取同步模式失败: {error}"));
                SyncMode::Manual
            })
        })
        .unwrap_or(SyncMode::Manual);
    state.selected_sync_mode.set(mode);
    state.updating_sync_mode_dropdown.set(true);
    state.mode_dropdown.set_selected(dropdown_index_from_sync_mode(mode));
    state.updating_sync_mode_dropdown.set(false);
}

pub(in crate::app) fn start_manual_one_time_sync_for_account(
    state: Rc<AppState>,
    account: Account,
) {
    start_one_time_sync(state, account, SyncStartMode::Normal);
}

pub(in crate::app) fn start_or_stop_manual_one_time_sync_for_account(
    state: Rc<AppState>,
    account: Account,
) {
    if is_sync_running(&state, &account.id) {
        stop_sync(&state, &account.id, "停止");
    } else {
        start_manual_one_time_sync_for_account(state, account);
    }
}

pub(in crate::app) fn start_or_stop_auto_sync_for_account(state: Rc<AppState>, account: Account) {
    if is_monitor_running(&state, &account.id) {
        stop_monitor(&state, &account.id);
    } else {
        start_monitor_for_account(state, account);
    }
}

pub(in crate::app) fn start_forced_one_time_sync_for_account(
    state: Rc<AppState>,
    account: Account,
) {
    start_one_time_sync(state, account, SyncStartMode::Force);
}

pub(in crate::app) fn start_resync_for_account(state: Rc<AppState>, account: Account) {
    start_one_time_sync(state, account, SyncStartMode::Resync);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncStartMode {
    Normal,
    Force,
    Resync,
}

fn start_one_time_sync(state: Rc<AppState>, account: Account, mode: SyncStartMode) {
    if is_sync_running(&state, &account.id) {
        show_toast(&state, "同步正在运行，请等待完成");
        return;
    }
    if !is_authenticated(&account) {
        show_toast(&state, "账号尚未完成认证");
        return;
    }
    if !ensure_client_ready(&state) {
        return;
    }
    if is_monitor_running(&state, &account.id) {
        show_toast(&state, "持续同步运行中，不能同时执行一次同步");
        return;
    }
    if !begin_operation(&state, &account.id, OperationKind::OneTimeSync) {
        return;
    }
    state.transfers.clear();
    refresh_content(&state);
    let account_id = account.id.clone();
    let command = onedrive_command(&state);
    let sender = state.sender.clone();
    let start_result = match mode {
        SyncStartMode::Normal => start_sync(account, command, sender),
        SyncStartMode::Force => start_forced_sync(account, command, sender),
        SyncStartMode::Resync => start_resync(account, command, sender),
    };
    match start_result {
        Ok(handle) => {
            state
                .operation_handles
                .borrow_mut()
                .insert(account_id, handle);
        }
        Err(error) => {
            finish_operation(&state, &account_id);
            update_account_status(
                &state,
                &account_id,
                AccountStatus::Error(format!("启动同步失败: {error}")),
            );
        }
    }
}

pub(in crate::app) fn start_preview_for_account(state: Rc<AppState>, account: Account) {
    if !is_authenticated(&account) {
        show_toast(&state, "账号尚未完成认证");
        return;
    }
    if !ensure_client_ready(&state) {
        return;
    }
    if is_sync_running(&state, &account.id) || is_monitor_running(&state, &account.id) {
        show_toast(&state, "同步运行中，不能同时预览");
        return;
    }
    if !begin_operation(&state, &account.id, OperationKind::Preview) {
        return;
    }

    state.previews.borrow_mut().remove(&account.id);
    state.transfers.clear();
    refresh_content(&state);

    match start_preview(
        account.clone(),
        onedrive_command(&state),
        state.sender.clone(),
    ) {
        Ok(handle) => {
            state
                .operation_handles
                .borrow_mut()
                .insert(account.id.clone(), handle);
        }
        Err(error) => {
            finish_operation(&state, &account.id);
            show_toast(&state, &format!("启动预览失败: {error}"));
        }
    }
}

pub(in crate::app) fn start_or_stop_preview_for_account(state: Rc<AppState>, account: Account) {
    if matches!(
        super::events::operation(&state, &account.id).map(|operation| operation.kind),
        Some(OperationKind::Preview)
    ) && is_sync_running(&state, &account.id)
    {
        stop_sync(&state, &account.id, "停止");
    } else {
        start_preview_for_account(state, account);
    }
}

pub(in crate::app) fn connect_preview_row_actions(state: Rc<AppState>) {
    let accept_state = Rc::clone(&state);
    state
        .transfers
        .connect_preview_accept(move |account_id, change_id| {
            apply_preview_change(Rc::clone(&accept_state), &account_id, &change_id);
        });

    let dismiss_state = Rc::clone(&state);
    state
        .transfers
        .connect_preview_dismiss(move |account_id, change_id| {
            if let Some(previews) = dismiss_state.previews.borrow_mut().get_mut(&account_id) {
                previews.remove(&change_id);
            }
            dismiss_state
                .transfers
                .dismiss_preview(&account_id, &change_id);
            show_toast(&dismiss_state, "已放弃该变更");
        });
}

fn apply_preview_change(state: Rc<AppState>, account_id: &str, change_id: &str) {
    let Some(account) = account_by_id(&state, account_id) else {
        show_toast(&state, "该账号已不存在");
        return;
    };
    let applying_key = (account_id.to_string(), change_id.to_string());
    if state
        .applying_preview_changes
        .borrow()
        .contains(&applying_key)
    {
        show_toast(&state, "该变更正在应用");
        return;
    }
    let Some(change) = state
        .previews
        .borrow()
        .get(account_id)
        .and_then(|previews| previews.get(change_id))
        .cloned()
    else {
        show_toast(&state, "该预览变更已不存在");
        return;
    };
    state
        .applying_preview_changes
        .borrow_mut()
        .insert(applying_key);
    state.transfers.mark_preview_applying(account_id, change_id);
    start_apply_preview_change(
        account,
        change,
        onedrive_command(&state),
        state.sender.clone(),
    );
}

fn account_by_id(state: &AppState, account_id: &str) -> Option<Account> {
    state
        .accounts
        .borrow()
        .iter()
        .find(|account| account.id == account_id)
        .cloned()
}

pub(in crate::app) fn start_monitor_for_account(state: Rc<AppState>, account: Account) {
    if is_monitor_running(&state, &account.id) {
        stop_monitor(&state, &account.id);
        return;
    }
    if !is_authenticated(&account) {
        show_toast(&state, "账号尚未完成认证");
        return;
    }
    if !ensure_client_ready(&state) {
        return;
    }
    if !begin_operation(&state, &account.id, OperationKind::Monitor) {
        return;
    }
    match start_monitor(
        account.clone(),
        onedrive_command(&state),
        state.sender.clone(),
    ) {
        Ok(handle) => {
            state
                .operation_handles
                .borrow_mut()
                .insert(account.id.clone(), handle);
            state.transfers.clear();
            refresh_content(&state);
        }
        Err(error) => {
            finish_operation(&state, &account.id);
            show_toast(&state, &format!("启动持续同步失败: {error}"));
        }
    }
}

pub(in crate::app) fn open_sync_dir_for_account(
    state: &AppState,
    account: &crate::profile::Account,
) {
    let path = crate::utils::expand_home(&account.sync_dir);
    if !path.exists() {
        show_toast(state, &format!("同步目录不存在: {}", path.display()));
        return;
    }

    let file = gio::File::for_path(path);
    if let Err(error) =
        gio::AppInfo::launch_default_for_uri(&file.uri(), None::<&gio::AppLaunchContext>)
    {
        show_toast(state, &format!("打开同步目录失败: {error}"));
    }
}
