use super::{
    events::{active_operation, is_monitor_running, is_sync_running},
    layout::{ACCOUNT_CONTEXT_MENU_WIDTH, build_profile_context_popover},
    state::{ActiveOperation, AppState},
    status::{account_label, status_detail, status_label, status_title},
    widgets::set_command_button_content,
};
use crate::account::{Account, AccountStatus};
use adw::prelude::*;

pub(in crate::app) fn rebuild_profile_list(state: &AppState) {
    while let Some(child) = state.profile_list.first_child() {
        state.profile_list.remove(&child);
    }

    for account in state.accounts.borrow().iter() {
        state.profile_list.append(&build_profile_row(account));
    }

    if !state.accounts.borrow().is_empty() {
        let selected = state
            .selected_index
            .get()
            .min(state.accounts.borrow().len().saturating_sub(1));
        state.selected_index.set(selected);
        if let Some(row) = state.profile_list.row_at_index(selected as i32) {
            state.profile_list.select_row(Some(&row));
        }
    }
}

pub(in crate::app) fn build_profile_row(account: &Account) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let action_row = adw::ActionRow::builder()
        .title(&account.name)
        .subtitle(format!(
            "{} · {}",
            account_label(account),
            status_label(&account.status)
        ))
        .build();

    action_row.add_prefix(&gtk::Image::from_icon_name("avatar-default-symbolic"));
    row.set_child(Some(&action_row));

    let account_menu = build_profile_context_popover();
    account_menu.set_parent(&row);

    let menu_clone = account_menu.clone();
    row.connect_destroy(move |_| {
        menu_clone.unparent();
    });

    let click = gtk::GestureClick::builder().button(3).build();
    click.connect_pressed(move |_, _, x, y| {
        let click_bounds =
            gtk::gdk::Rectangle::new(x as i32, y as i32, ACCOUNT_CONTEXT_MENU_WIDTH, 1);
        account_menu.set_pointing_to(Some(&click_bounds));
        account_menu.popup();
    });
    row.add_controller(click);

    row
}

pub(in crate::app) fn refresh_content(state: &AppState) {
    let Some(account) = state.selected_account() else {
        state.title.set_title("OneSync");
        state.title.set_subtitle("未选择账号");
        state.status_title.set_label("未配置");
        state.status_detail.set_label("添加账号后开始认证");
        state.account_menu_button.set_sensitive(false);
        state.one_time_sync_button.set_sensitive(false);
        state.monitor_button.set_sensitive(false);
        state.edit_button.set_sensitive(false);
        state.transfers.clear();
        return;
    };

    state.title.set_title(&account.name);
    state.title.set_subtitle(&account_label(&account));
    state.account_menu_button.set_sensitive(true);
    state.status_title.set_label(status_title(&account.status));
    state.status_detail.set_label(&status_detail(&account));
    let client_ready = state.client_check.borrow().is_ready();
    let operation = active_operation(state, &account.id);
    let stopping_sync = matches!(operation, Some(ActiveOperation::StoppingSync));
    let stopping_monitor = matches!(operation, Some(ActiveOperation::StoppingMonitor));
    let syncing =
        matches!(account.status, AccountStatus::Syncing) || is_sync_running(state, &account.id);
    let monitoring = matches!(account.status, AccountStatus::Monitoring)
        || is_monitor_running(state, &account.id);

    if stopping_sync {
        set_command_button_content(
            &state.one_time_sync_button,
            "process-stop-symbolic",
            "正在停止",
        );
        state.one_time_sync_button.set_sensitive(false);
    } else if syncing {
        set_command_button_content(
            &state.one_time_sync_button,
            "process-stop-symbolic",
            "停止同步",
        );
        state.one_time_sync_button.set_sensitive(true);
    } else {
        set_command_button_content(
            &state.one_time_sync_button,
            "view-refresh-symbolic",
            "一次同步",
        );
        state.one_time_sync_button.set_sensitive(
            client_ready
                && operation.is_none()
                && matches!(account.status, AccountStatus::Authenticated),
        );
    }

    state.monitor_button.set_sensitive(
        !stopping_monitor
            && matches!(
                account.status,
                AccountStatus::Authenticated | AccountStatus::Monitoring
            )
            && (client_ready || monitoring)
            && (operation.is_none() || monitoring),
    );
    state.edit_button.set_sensitive(true);
    if matches!(account.status, AccountStatus::Monitoring) {
        set_command_button_content(
            &state.monitor_button,
            "media-playback-stop-symbolic",
            "停止持续同步",
        );
    } else if stopping_monitor {
        set_command_button_content(&state.monitor_button, "process-stop-symbolic", "正在停止");
    } else {
        set_command_button_content(
            &state.monitor_button,
            "media-playback-start-symbolic",
            "持续同步",
        );
    }
}

pub(in crate::app) fn show_toast(state: &AppState, message: &str) {
    state.toast_overlay.add_toast(adw::Toast::new(message));
}
