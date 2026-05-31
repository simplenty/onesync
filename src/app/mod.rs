mod auth;
mod confirm;
mod events;
mod layout;
mod list;
mod profile;
mod render;
mod state;
mod status;
mod widgets;

use crate::{
    account::{Account, AccountStatus, is_authenticated, load_store, save_accounts},
    onedrive::{
        ClientCheck, check_client, start_account_identity_lookup, start_forced_sync, start_monitor,
        start_sync,
    },
    settings::{DEFAULT_ONEDRIVE_COMMAND, load_onedrive_command},
};
use adw::prelude::*;
use events::{
    begin_active_operation, can_mutate_profile, ensure_client_ready, finish_active_operation,
    has_active_operation, install_backend_event_pump, is_monitor_running, is_sync_running,
    show_active_operation_toast, stop_all_monitors, stop_monitor, stop_sync,
};
use gtk::gio;
use gtk::glib;
use layout::{build_content_widgets, build_sidebar};
use list::TransferList;
use render::{rebuild_profile_list, refresh_content, show_toast};
use state::{ActiveOperation, AppState};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
};

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
    let accounts = match load_store() {
        Ok(store) => store.accounts,
        Err(error) => {
            eprintln!("failed to load account store: {error}");
            Vec::new()
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
        accounts: RefCell::new(accounts),
        selected_index: Cell::new(0),
        client_check: RefCell::new(ClientCheck::Unknown),
        onedrive_command: configured_onedrive_command,
        sender,
        receiver: RefCell::new(receiver),
        auth_panel: RefCell::new(None),
        syncs: RefCell::new(HashMap::new()),
        monitors: RefCell::new(HashMap::new()),
        active_operations: RefCell::new(HashMap::new()),
        toast_overlay: toast_overlay.clone(),
        window: window.clone(),
        profile_list,
        title: content_widgets.title,
        status_title: content_widgets.status_title,
        status_detail: content_widgets.status_detail,
        transfers: TransferList::new(content_widgets.files_list),
        account_menu_button: content_widgets.account_menu_button,
        settings_button: content_widgets.settings_button,
        one_time_sync_button: content_widgets.one_time_sync_button,
        monitor_button: content_widgets.monitor_button,
        edit_button: content_widgets.edit_button,
    });

    let sidebar = build_sidebar(Rc::clone(&state));
    connect_actions(Rc::clone(&state));
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
    if state.accounts.borrow().is_empty() {
        profile::show_add_account_dialog(Rc::clone(&state));
    }
}

fn connect_actions(state: Rc<AppState>) {
    state.settings_button.connect_clicked(|_| {});

    let one_time_sync_state = Rc::clone(&state);
    state.one_time_sync_button.connect_clicked(move |_| {
        let Some(account) = one_time_sync_state.selected_account() else {
            show_toast(&one_time_sync_state, "请先选择账号");
            return;
        };
        start_one_time_sync_for_account(Rc::clone(&one_time_sync_state), account);
    });

    let monitor_state = Rc::clone(&state);
    state.monitor_button.connect_clicked(move |_| {
        let Some(account) = monitor_state.selected_account() else {
            show_toast(&monitor_state, "请先选择账号");
            return;
        };
        start_monitor_for_account(Rc::clone(&monitor_state), account);
    });

    let edit_state = Rc::clone(&state);
    state.edit_button.connect_clicked(move |_| {
        edit_state.account_menu_button.popdown();
        let Some(account) = edit_state.selected_account() else {
            show_toast(&edit_state, "请先选择账号");
            return;
        };
        profile::show_edit_profile_dialog(Rc::clone(&edit_state), account);
    });
}

pub(in crate::app) fn start_one_time_sync_for_account(state: Rc<AppState>, account: Account) {
    start_one_time_sync(state, account, false);
}

pub(in crate::app) fn start_forced_one_time_sync_for_account(
    state: Rc<AppState>,
    account: Account,
) {
    start_one_time_sync(state, account, true);
}

fn start_one_time_sync(state: Rc<AppState>, account: Account, force_big_delete: bool) {
    if is_sync_running(&state, &account.id) {
        stop_sync(&state, &account.id);
        return;
    }
    if !is_authenticated(&account) {
        show_toast(&state, "账号尚未完成认证");
        return;
    }
    if !ensure_client_ready(&state) {
        return;
    }
    if has_active_operation(&state, &account.id) {
        show_active_operation_toast(&state, &account.id);
        return;
    }
    if is_monitor_running(&state, &account.id) {
        show_toast(&state, "持续同步运行中，不能同时执行一次同步");
        return;
    }
    if !begin_active_operation(&state, &account.id, ActiveOperation::Sync) {
        return;
    }
    update_account_status(&state, &account.id, AccountStatus::Syncing);
    state.transfers.clear();
    refresh_content(&state);
    let account_id = account.id.clone();
    let start_result = if force_big_delete {
        start_forced_sync(account, onedrive_command(&state), state.sender.clone())
    } else {
        start_sync(account, onedrive_command(&state), state.sender.clone())
    };
    match start_result {
        Ok(handle) => {
            state.syncs.borrow_mut().insert(account_id, handle);
        }
        Err(error) => {
            finish_active_operation(&state, &account_id);
            update_account_status(
                &state,
                &account_id,
                AccountStatus::Error(format!("启动同步失败: {error}")),
            );
        }
    }
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
    if has_active_operation(&state, &account.id) {
        show_active_operation_toast(&state, &account.id);
        return;
    }
    if matches!(account.status, AccountStatus::Syncing) {
        show_toast(&state, "一次同步运行中，请稍后再启动持续同步");
        return;
    }

    match start_monitor(
        account.clone(),
        onedrive_command(&state),
        state.sender.clone(),
    ) {
        Ok(handle) => {
            state
                .monitors
                .borrow_mut()
                .insert(account.id.clone(), handle);
            update_account_status(&state, &account.id, AccountStatus::Monitoring);
            state.transfers.clear();
            refresh_content(&state);
        }
        Err(error) => show_toast(&state, &format!("启动持续同步失败: {error}")),
    }
}

pub(in crate::app) fn open_sync_dir_for_account(
    state: &AppState,
    account: &crate::account::Account,
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

fn connect_shutdown(state: Rc<AppState>) {
    let window = state.window.clone();
    window.connect_close_request(move |_| {
        stop_all_monitors(&state);
        glib::Propagation::Proceed
    });
}

pub(in crate::app) fn close_auth_panel(state: &AppState, account_id: &str) {
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
    if let Some(account) = state
        .accounts
        .borrow_mut()
        .iter_mut()
        .find(|account| account.id == account_id)
    {
        account.status = status;
    }
    if let Err(error) = save_accounts(&state.accounts.borrow()) {
        show_toast(state, &format!("保存账号状态失败: {error}"));
    }
    rebuild_profile_list(state);
    refresh_content(state);
}

fn refresh_accounts_from_disk(state: &AppState) {
    for account in state.accounts.borrow_mut().iter_mut() {
        if is_authenticated(account) {
            account.status = AccountStatus::Authenticated;
        } else {
            account.status = AccountStatus::NeedsAuth;
        }
    }
}

fn start_missing_identity_lookups(state: &AppState) {
    let accounts: Vec<Account> = state
        .accounts
        .borrow()
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
