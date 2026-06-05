use super::{
    account_label,
    events::operation,
    layout::{ACCOUNT_CONTEXT_MENU_WIDTH, build_profile_context_popover},
    actions::{open_sync_dir_for_account, start_monitor_for_account, start_one_time_sync_for_account},
    actions::load_sync_mode_for_selected_profile,
    state::AppState,
    status_detail, status_label, status_title,
    widgets::set_command_button_content,
};
use crate::profile::{Account, AccountStatus};
use crate::operation::{OperationKind, OperationPhase};
use crate::operation::{CommandRuntime, ControlInput, controls_for};
use adw::prelude::*;
use std::rc::Rc;

pub(in crate::app) fn rebuild_profile_list(state: &Rc<AppState>) {
    while let Some(child) = state.profile_list.first_child() {
        state.profile_list.remove(&child);
    }

    for (index, account) in state.accounts.borrow().iter().enumerate() {
        state
            .profile_list
            .append(&build_profile_row(Rc::clone(state), account, index));
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

pub(in crate::app) fn build_profile_row(
    state: Rc<AppState>,
    account: &Account,
    index: usize,
) -> gtk::ListBoxRow {
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
    account_menu.popover.set_parent(&row);

    let sync_state = Rc::clone(&state);
    let sync_account = account.clone();
    let sync_popover = account_menu.popover.clone();
    account_menu.sync_once_button.connect_clicked(move |_| {
        sync_popover.popdown();
        start_one_time_sync_for_account(Rc::clone(&sync_state), sync_account.clone());
    });

    let monitor_state = Rc::clone(&state);
    let monitor_account = account.clone();
    let monitor_popover = account_menu.popover.clone();
    account_menu.monitor_button.connect_clicked(move |_| {
        monitor_popover.popdown();
        start_monitor_for_account(Rc::clone(&monitor_state), monitor_account.clone());
    });

    let open_state = Rc::clone(&state);
    let open_account = account.clone();
    let open_popover = account_menu.popover.clone();
    account_menu.open_sync_dir_button.connect_clicked(move |_| {
        open_popover.popdown();
        open_sync_dir_for_account(&open_state, &open_account);
    });

    let menu_clone = account_menu.popover.clone();
    row.connect_destroy(move |_| {
        menu_clone.unparent();
    });

    let click_state = Rc::clone(&state);
    let click_row = row.clone();
    let popover = account_menu.popover.clone();
    let click = gtk::GestureClick::builder().button(3).build();
    click.connect_pressed(move |_, _, x, y| {
        click_state.selected_index.set(index);
        click_state.profile_list.select_row(Some(&click_row));
        load_sync_mode_for_selected_profile(&click_state);
        refresh_content(&click_state);
        let click_bounds =
            gtk::gdk::Rectangle::new(x as i32, y as i32, ACCOUNT_CONTEXT_MENU_WIDTH, 1);
        popover.set_pointing_to(Some(&click_bounds));
        popover.popup();
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
        state.sync_button.set_sensitive(false);
        state.preview_button.set_sensitive(false);
        state.preview_button.set_visible(true);
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
    let runtime = match operation(state, &account.id) {
        Some(operation) => match (operation.kind, operation.phase) {
            (OperationKind::OneTimeSync, OperationPhase::Running) => {
                CommandRuntime::RunningManualSync
            }
            (OperationKind::OneTimeSync, OperationPhase::Stopping) => {
                CommandRuntime::StoppingManualSync
            }
            (OperationKind::Preview, OperationPhase::Running) => CommandRuntime::RunningPreview,
            (OperationKind::Preview, OperationPhase::Stopping) => CommandRuntime::StoppingPreview,
            (OperationKind::Monitor, OperationPhase::Running) => CommandRuntime::RunningMonitor,
            (OperationKind::Monitor, OperationPhase::Stopping) => CommandRuntime::StoppingMonitor,
            (OperationKind::Authentication | OperationKind::ApplyPreviewChange | OperationKind::Reconcile, _) => CommandRuntime::Blocked,
        },
        None => CommandRuntime::Idle,
    };
    let controls = controls_for(ControlInput {
        mode: state.selected_sync_mode.get(),
        runtime,
        authenticated: matches!(account.status, AccountStatus::Authenticated),
        client_ready,
    });

    state.sync_button.set_visible(controls.sync.visible);
    state.sync_button.set_sensitive(controls.sync.sensitive);
    set_command_button_content(&state.sync_button, controls.sync.icon, controls.sync.label);
    state.preview_button.set_visible(controls.preview.visible);
    state
        .preview_button
        .set_sensitive(controls.preview.sensitive);
    set_command_button_content(
        &state.preview_button,
        controls.preview.icon,
        controls.preview.label,
    );
    state.edit_button.set_sensitive(true);
}

pub(in crate::app) fn show_toast(state: &AppState, message: &str) {
    state.toast_overlay.dismiss_all();
    state.toast_overlay.add_toast(adw::Toast::new(message));
}
