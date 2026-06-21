mod actions;
mod dialogs;
mod events;
mod layout;
mod present;
mod render;
mod state;
mod widgets;

use crate::{
    adapter::{
        graph::start_account_identity_lookup,
        onedrive::{check_client, stop_operation},
    },
    event::ClientCheck,
    operation::OperationRegistry,
    profile::{
        Account, AccountStatus, AccountStore, DEFAULT_ONEDRIVE_COMMAND, SyncMode, is_authenticated,
        load_onedrive_command, load_store, save_profile_sync_mode,
    },
};
use actions::{
    connect_preview_row_actions, start_or_stop_auto_sync_for_account,
    start_or_stop_manual_one_time_sync_for_account, start_or_stop_preview_for_account,
};
use adw::prelude::*;
use events::{
    begin_operation, can_mutate_profile, finish_operation, install_backend_event_pump,
    stop_all_monitors,
};
use gtk::glib;
use layout::{build_content_widgets, build_sidebar};
pub(in crate::app) use present::*;
use render::{rebuild_profile_list, refresh_content, show_toast};
use state::AppState;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
};
use widgets::TransferList;

const APP_ID: &str = "io.github.onesync.Demo";

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("OneSync")
        .default_width(1080)
        .default_height(720)
        .build();
    window.set_size_request(860, 560);

    let (sender, receiver) = mpsc::channel();
    let configured_onedrive_command = match load_onedrive_command() {
        Ok(command) => command,
        Err(error) => {
            eprintln!("failed to load GUI settings: {error}");
            DEFAULT_ONEDRIVE_COMMAND.to_string()
        }
    };
    let store = match load_store() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("failed to load account store: {error}");
            AccountStore::default()
        }
    };

    let toast_overlay = adw::ToastOverlay::new();
    let split_view = adw::OverlaySplitView::builder()
        .sidebar_width_fraction(0.28)
        .min_sidebar_width(260.0)
        .max_sidebar_width(340.0)
        .build();

    let profile_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["boxed-list"])
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(12)
        .margin_end(12)
        .build();

    let (content, content_widgets) = build_content_widgets();
    let state = Rc::new(AppState {
        store: RefCell::new(store),
        selected_index: Cell::new(0),
        client_check: RefCell::new(ClientCheck::Unknown),
        onedrive_command: configured_onedrive_command,
        sender,
        receiver: RefCell::new(receiver),
        auth_panel: RefCell::new(None),
        operation_handles: RefCell::new(HashMap::new()),
        operations: RefCell::new(OperationRegistry::default()),
        previews: RefCell::new(HashMap::new()),
        applying_preview_changes: RefCell::new(std::collections::HashSet::new()),
        pending_confirmation: RefCell::new(None),
        toast_overlay: toast_overlay.clone(),
        window: window.clone(),
        profile_list,
        title: content_widgets.title,
        status_title: content_widgets.status_title,
        status_detail: content_widgets.status_detail,
        transfers: TransferList::new(content_widgets.files_list),
        account_menu_button: content_widgets.account_menu_button,
        settings_button: content_widgets.settings_button,
        mode_dropdown: content_widgets.mode_dropdown,
        selected_sync_mode: Cell::new(SyncMode::Manual),
        updating_sync_mode_dropdown: Cell::new(false),
        sync_button: content_widgets.sync_button,
        preview_button: content_widgets.preview_button,
        edit_button: content_widgets.edit_button,
        auth_button: content_widgets.auth_button,
    });

    let sidebar = build_sidebar(Rc::clone(&state));
    connect_actions(Rc::clone(&state));
    connect_preview_row_actions(Rc::clone(&state));
    connect_shutdown(Rc::clone(&state));
    refresh_accounts_from_disk(&state);
    rebuild_profile_list(&state);
    refresh_content(&state);
    install_backend_event_pump(Rc::clone(&state));
    start_missing_identity_lookups(&state);
    check_client(onedrive_command(&state), state.sender.clone());

    split_view.set_sidebar(Some(&sidebar));
    split_view.set_content(Some(&content));
    toast_overlay.set_child(Some(&split_view));

    window.set_content(Some(&toast_overlay));
    window.present();
    if state.store.borrow().accounts().is_empty() {
        dialogs::profile::show_add_account_dialog(Rc::clone(&state));
    }
}

fn connect_actions(state: Rc<AppState>) {
    state.settings_button.connect_clicked(|_| {});

    let mode_state = Rc::clone(&state);
    state
        .mode_dropdown
        .connect_selected_notify(move |dropdown| {
            if mode_state.updating_sync_mode_dropdown.get() {
                return;
            }
            let mode = sync_mode_from_dropdown_index(dropdown.selected());
            mode_state.selected_sync_mode.set(mode);
            if let Some(account_id) = mode_state.selected_account_id()
                && let Err(error) = save_profile_sync_mode(&account_id, mode)
            {
                show_toast(&mode_state, &format!("保存同步模式失败: {error}"));
            }
            refresh_content(&mode_state);
        });

    let sync_state = Rc::clone(&state);
    state.sync_button.connect_clicked(move |_| {
        let Some(account) = sync_state.selected_account() else {
            show_toast(&sync_state, "请先选择账号");
            return;
        };
        match sync_state.selected_sync_mode.get() {
            SyncMode::Automatic => {
                start_or_stop_auto_sync_for_account(Rc::clone(&sync_state), account)
            }
            SyncMode::Manual => {
                start_or_stop_manual_one_time_sync_for_account(Rc::clone(&sync_state), account)
            }
        }
    });

    let preview_state = Rc::clone(&state);
    state.preview_button.connect_clicked(move |_| {
        let Some(account) = preview_state.selected_account() else {
            show_toast(&preview_state, "请先选择账号");
            return;
        };
        start_or_stop_preview_for_account(Rc::clone(&preview_state), account);
    });

    let edit_state = Rc::clone(&state);
    state.edit_button.connect_clicked(move |_| {
        edit_state.account_menu_button.popdown();
        let Some(account) = edit_state.selected_account() else {
            show_toast(&edit_state, "请先选择账号");
            return;
        };
        dialogs::profile::show_edit_profile_dialog(Rc::clone(&edit_state), account);
    });

    let auth_state = Rc::clone(&state);
    state.auth_button.connect_clicked(move |_| {
        let Some(account) = auth_state.selected_account() else {
            show_toast(&auth_state, "请先选择账号");
            return;
        };
        if !can_mutate_profile(&auth_state, &account) {
            return;
        }
        dialogs::auth::show_auth_dialog(Rc::clone(&auth_state), account);
    });
}

fn connect_shutdown(state: Rc<AppState>) {
    let window = state.window.clone();
    window.connect_close_request(move |_| {
        stop_all_monitors(&state);
        glib::Propagation::Proceed
    });
}

pub(in crate::app) fn close_auth_panel(state: &AppState, account_id: &str) {
    let handle = state.operation_handles.borrow().get(account_id).cloned();
    if let Some(handle) = handle {
        let _ = stop_operation(&handle);
        finish_operation(state, account_id);
        state.operation_handles.borrow_mut().remove(account_id);
    }

    let panel = {
        let should_clear = state
            .auth_panel
            .borrow()
            .as_ref()
            .is_some_and(|panel| panel.account_id == account_id);
        if should_clear {
            state.auth_panel.replace(None)
        } else {
            None
        }
    };

    if let Some(panel) = panel {
        panel.window.destroy();
    }
}

pub(in crate::app) fn update_account_status(
    state: &Rc<AppState>,
    account_id: &str,
    status: AccountStatus,
) {
    if let Err(error) = state.store.borrow_mut().update_status(account_id, status) {
        show_toast(state, &format!("保存账号状态失败: {error}"));
    }
    rebuild_profile_list(state);
    refresh_content(state);
}

fn refresh_accounts_from_disk(state: &AppState) {
    for account in state.store.borrow_mut().accounts_mut() {
        if is_authenticated(account) {
            account.status = AccountStatus::Authenticated;
        } else {
            account.status = AccountStatus::NeedsAuth;
        }
    }
}

fn start_missing_identity_lookups(state: &AppState) {
    let accounts: Vec<Account> = state
        .store
        .borrow()
        .accounts()
        .iter()
        .filter(|account| is_authenticated(account) && needs_identity_lookup(account))
        .cloned()
        .collect();
    for account in accounts {
        start_account_identity_lookup(account, state.sender.clone());
    }
}

fn needs_identity_lookup(account: &Account) -> bool {
    let name = account.name.trim();
    account.email.trim().is_empty()
        || name.is_empty()
        || name == "OneDrive"
        || name.starts_with("OneDrive ")
        || name == account.email
}

pub(in crate::app) fn onedrive_command(state: &AppState) -> String {
    state.onedrive_command.clone()
}

pub(in crate::app) fn sync_mode_from_dropdown_index(index: u32) -> SyncMode {
    match index {
        0 => SyncMode::Manual,
        1 => SyncMode::Automatic,
        _ => SyncMode::Manual,
    }
}

pub(in crate::app) fn dropdown_index_from_sync_mode(mode: SyncMode) -> u32 {
    match mode {
        SyncMode::Manual => 0,
        SyncMode::Automatic => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_mode_maps_to_dropdown_indices() {
        assert_eq!(sync_mode_from_dropdown_index(0), SyncMode::Manual);
        assert_eq!(sync_mode_from_dropdown_index(1), SyncMode::Automatic);
        assert_eq!(sync_mode_from_dropdown_index(99), SyncMode::Manual);
        assert_eq!(dropdown_index_from_sync_mode(SyncMode::Manual), 0);
        assert_eq!(dropdown_index_from_sync_mode(SyncMode::Automatic), 1);
    }
}
