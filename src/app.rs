use crate::{
    account::{
        Account, AccountStatus, auth_response_path, create_account, default_account_name,
        default_sync_dir, is_authenticated, load_store, save_accounts,
    },
    onedrive::{
        BackendEvent, ClientCheck, MonitorHandle, SyncHandle, check_client, start_authentication,
        start_logout, start_monitor, start_sync, stop_handle, stop_monitor_handle,
    },
    settings::load_onedrive_command,
    ui::{TransferList, command_button, form_row, set_command_button_content},
};
use adw::prelude::*;
use gtk::{Align, glib};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    fs,
    path::Path,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

const APP_ID: &str = "io.github.onesync.Demo";
const ACCOUNT_CONTEXT_MENU_WIDTH: i32 = 160;

struct AppState {
    accounts: RefCell<Vec<Account>>,
    selected_index: Cell<usize>,
    client_check: RefCell<ClientCheck>,
    onedrive_command: String,
    sender: mpsc::Sender<BackendEvent>,
    receiver: RefCell<mpsc::Receiver<BackendEvent>>,
    auth_panel: RefCell<Option<AuthPanel>>,
    syncs: RefCell<HashMap<String, SyncHandle>>,
    monitors: RefCell<HashMap<String, MonitorHandle>>,
    active_operations: RefCell<HashMap<String, ActiveOperation>>,
    toast_overlay: adw::ToastOverlay,
    window: adw::ApplicationWindow,
    profile_list: gtk::ListBox,
    title: adw::WindowTitle,
    status_title: gtk::Label,
    status_detail: gtk::Label,
    transfers: TransferList,
    account_menu_button: gtk::MenuButton,
    settings_button: gtk::Button,
    one_time_sync_button: gtk::Button,
    monitor_button: gtk::Button,
    edit_button: gtk::Button,
}

#[derive(Clone)]
struct AuthPanel {
    account_id: String,
    window: adw::Window,
    status_label: gtk::Label,
    auth_url_entry: gtk::Entry,
}

#[derive(Clone, Copy)]
enum ActiveOperation {
    Authentication,
    Sync,
    StoppingSync,
    StoppingMonitor,
    Logout,
}

impl ActiveOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Authentication => "认证",
            Self::Sync => "一次同步",
            Self::StoppingSync => "停止同步",
            Self::StoppingMonitor => "停止持续同步",
            Self::Logout => "退出登录",
        }
    }
}

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
            "onedrive".to_string()
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
    check_client(onedrive_command(&state), state.sender.clone());

    split_view.set_sidebar(Some(&sidebar));
    split_view.set_content(Some(&content));
    toast_overlay.set_child(Some(&split_view));

    window.set_content(Some(&toast_overlay));
    window.present();
}

struct ContentWidgets {
    title: adw::WindowTitle,
    status_title: gtk::Label,
    status_detail: gtk::Label,
    files_list: gtk::ListBox,
    account_menu_button: gtk::MenuButton,
    settings_button: gtk::Button,
    one_time_sync_button: gtk::Button,
    monitor_button: gtk::Button,
    edit_button: gtk::Button,
}

fn build_sidebar(state: Rc<AppState>) -> adw::ToolbarView {
    let header = adw::HeaderBar::new();
    let add_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("添加账号")
        .build();
    header.pack_end(&add_button);

    let title = adw::WindowTitle::builder().title("Profiles").build();
    header.set_title_widget(Some(&title));

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);

    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    sidebar_box.append(&state.profile_list);

    let select_state = Rc::clone(&state);
    state.profile_list.connect_row_selected(move |_, row| {
        let Some(row) = row else {
            return;
        };
        let next_index = row.index() as usize;
        let changed_account = select_state.selected_index.get() != next_index;
        select_state.selected_index.set(next_index);
        refresh_content(&select_state);
        if changed_account {
            select_state.transfers.clear();
        }
    });

    let add_state = Rc::clone(&state);
    add_button.connect_clicked(move |_| {
        show_add_account_dialog(Rc::clone(&add_state));
    });

    toolbar_view.set_content(Some(&sidebar_box));
    toolbar_view
}

fn build_content_widgets() -> (adw::ToolbarView, ContentWidgets) {
    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::builder()
        .title("OneSync")
        .subtitle("未选择账号")
        .build();
    header.set_title_widget(Some(&title));

    let settings_button = gtk::Button::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("设置")
        .build();
    header.pack_end(&settings_button);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let summary_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .build();

    let status_line = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let status_title = gtk::Label::builder()
        .label("未配置")
        .halign(Align::Start)
        .hexpand(true)
        .css_classes(["title-3"])
        .build();
    status_line.append(&status_title);

    let status_detail = gtk::Label::builder()
        .label("添加账号后开始认证")
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let one_time_sync_button = command_button("view-refresh-symbolic", "一次同步");
    let monitor_button = command_button("media-playback-start-symbolic", "持续同步");
    let edit_button = command_button("document-edit-symbolic", "编辑 Profile");
    actions.append(&one_time_sync_button);
    actions.append(&monitor_button);

    let account_menu_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("账户操作")
        .build();
    account_menu_button.set_popover(Some(&build_account_actions_popover(&[(
        &edit_button,
        "document-edit-symbolic",
        "编辑 Profile",
    )])));
    actions.append(&account_menu_button);

    summary_box.append(&status_line);
    summary_box.append(&status_detail);
    summary_box.append(&actions);
    page.append(&summary_box);

    let files_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    let files_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .min_content_height(260)
        .build();
    files_scrolled.set_child(Some(&files_list));
    page.append(&files_scrolled);

    toolbar_view.set_content(Some(&page));
    (
        toolbar_view,
        ContentWidgets {
            title,
            status_title,
            status_detail,
            files_list,
            account_menu_button,
            settings_button,
            one_time_sync_button,
            monitor_button,
            edit_button,
        },
    )
}

fn connect_actions(state: Rc<AppState>) {
    state.settings_button.connect_clicked(|_| {});

    let one_time_sync_state = Rc::clone(&state);
    state.one_time_sync_button.connect_clicked(move |_| {
        let Some(account) = selected_account(&one_time_sync_state) else {
            show_toast(&one_time_sync_state, "请先选择账号");
            return;
        };
        if is_sync_running(&one_time_sync_state, &account.id) {
            stop_sync(&one_time_sync_state, &account.id);
            return;
        }
        if !is_authenticated(&account) {
            show_toast(&one_time_sync_state, "账号尚未完成认证");
            return;
        }
        if !ensure_client_ready(&one_time_sync_state) {
            return;
        }
        if has_active_operation(&one_time_sync_state, &account.id) {
            show_active_operation_toast(&one_time_sync_state, &account.id);
            return;
        }
        if is_monitor_running(&one_time_sync_state, &account.id) {
            show_toast(&one_time_sync_state, "持续同步运行中，不能同时执行一次同步");
            return;
        }
        if !begin_active_operation(&one_time_sync_state, &account.id, ActiveOperation::Sync) {
            return;
        }
        update_account_status(&one_time_sync_state, &account.id, AccountStatus::Syncing);
        one_time_sync_state.transfers.clear();
        refresh_content(&one_time_sync_state);
        let account_id = account.id.clone();
        match start_sync(
            account,
            onedrive_command(&one_time_sync_state),
            one_time_sync_state.sender.clone(),
        ) {
            Ok(handle) => {
                one_time_sync_state
                    .syncs
                    .borrow_mut()
                    .insert(account_id, handle);
            }
            Err(error) => {
                finish_active_operation(&one_time_sync_state, &account_id);
                update_account_status(
                    &one_time_sync_state,
                    &account_id,
                    AccountStatus::Error(format!("启动同步失败: {error}")),
                );
            }
        }
    });

    let monitor_state = Rc::clone(&state);
    state.monitor_button.connect_clicked(move |_| {
        let Some(account) = selected_account(&monitor_state) else {
            show_toast(&monitor_state, "请先选择账号");
            return;
        };
        if is_monitor_running(&monitor_state, &account.id) {
            stop_monitor(&monitor_state, &account.id);
            return;
        }
        if !is_authenticated(&account) {
            show_toast(&monitor_state, "账号尚未完成认证");
            return;
        }
        if !ensure_client_ready(&monitor_state) {
            return;
        }
        if has_active_operation(&monitor_state, &account.id) {
            show_active_operation_toast(&monitor_state, &account.id);
            return;
        }
        if matches!(account.status, AccountStatus::Syncing) {
            show_toast(&monitor_state, "一次同步运行中，请稍后再启动持续同步");
            return;
        }

        match start_monitor(
            account.clone(),
            onedrive_command(&monitor_state),
            monitor_state.sender.clone(),
        ) {
            Ok(handle) => {
                monitor_state
                    .monitors
                    .borrow_mut()
                    .insert(account.id.clone(), handle);
                update_account_status(&monitor_state, &account.id, AccountStatus::Monitoring);
                monitor_state.transfers.clear();
                refresh_content(&monitor_state);
            }
            Err(error) => show_toast(&monitor_state, &format!("启动持续同步失败: {error}")),
        }
    });

    let edit_state = Rc::clone(&state);
    state.edit_button.connect_clicked(move |_| {
        let Some(account) = selected_account(&edit_state) else {
            show_toast(&edit_state, "请先选择账号");
            return;
        };
        show_edit_profile_dialog(Rc::clone(&edit_state), account);
    });
}

fn connect_shutdown(state: Rc<AppState>) {
    let window = state.window.clone();
    window.connect_close_request(move |_| {
        stop_all_monitors(&state);
        glib::Propagation::Proceed
    });
}

fn install_backend_event_pump(state: Rc<AppState>) {
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
                if selected_account(state).is_some_and(|account| account.id == account_id) {
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
                } else if let Some(account) = selected_account(state) {
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
                    show_warning_window(state, "需要确认", kind.user_message());
                } else if requested_stop {
                    show_toast(state, "同步已停止");
                } else if success {
                    show_toast(state, "同步完成");
                } else if let Some(account) = selected_account(state) {
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
                if selected_account(state).is_some_and(|account| account.id == account_id) {
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
                    show_warning_window(state, "需要确认", kind.user_message());
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

fn stop_monitor(state: &AppState, account_id: &str) {
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

fn stop_sync(state: &AppState, account_id: &str) {
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

fn stop_all_monitors(state: &AppState) {
    let sync_handles: Vec<SyncHandle> = state.syncs.borrow().values().cloned().collect();
    for handle in sync_handles {
        let _ = stop_handle(&handle);
    }
    let handles: Vec<MonitorHandle> = state.monitors.borrow().values().cloned().collect();
    for handle in handles {
        let _ = stop_monitor_handle(&handle);
    }
}

fn is_monitor_running(state: &AppState, account_id: &str) -> bool {
    state.monitors.borrow().contains_key(account_id)
}

fn is_sync_running(state: &AppState, account_id: &str) -> bool {
    state.syncs.borrow().contains_key(account_id)
}

fn has_active_operation(state: &AppState, account_id: &str) -> bool {
    state.active_operations.borrow().contains_key(account_id)
}

fn active_operation(state: &AppState, account_id: &str) -> Option<ActiveOperation> {
    state.active_operations.borrow().get(account_id).copied()
}

fn can_mutate_profile(state: &AppState, account: &Account) -> bool {
    !matches!(
        account.status,
        AccountStatus::Authenticating | AccountStatus::Syncing | AccountStatus::Monitoring
    ) && !has_active_operation(state, &account.id)
        && !is_sync_running(state, &account.id)
        && !is_monitor_running(state, &account.id)
}

fn remove_confirmation_matches(expected_name: &str, input: &str) -> bool {
    input == expected_name
}

fn begin_active_operation(state: &AppState, account_id: &str, operation: ActiveOperation) -> bool {
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

fn finish_active_operation(state: &AppState, account_id: &str) {
    state.active_operations.borrow_mut().remove(account_id);
    refresh_content(state);
}

fn show_active_operation_toast(state: &AppState, account_id: &str) {
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
        show_auth_dialog(state, account);
    }
}

fn show_auth_dialog(state: Rc<AppState>, account: Account) {
    let existing_panel = state.auth_panel.borrow().clone();
    if let Some(panel) = existing_panel {
        if panel.account_id == account.id {
            panel.window.present();
            return;
        }
        close_auth_panel(&state, &panel.account_id);
    }

    let dialog = adw::Window::builder()
        .title("账号认证")
        .transient_for(&state.window)
        .modal(true)
        .default_width(620)
        .build();

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let header = adw::HeaderBar::new();
    let close_button = gtk::Button::with_label("关闭");
    header.pack_start(&close_button);
    root.append(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&content);

    let status_label = gtk::Label::builder()
        .label("点击生成认证链接，然后在浏览器完成 Microsoft 登录")
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    content.append(&status_label);

    let auth_url_entry = gtk::Entry::builder()
        .editable(false)
        .hexpand(true)
        .placeholder_text("生成后这里会显示认证链接")
        .build();
    content.append(&form_row("认证链接", &auth_url_entry));

    let auth_response_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("粘贴浏览器最终跳转到的完整 redirect URI")
        .build();
    content.append(&form_row("回调 URI", &auth_response_entry));

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(Align::End)
        .build();
    let start_button = gtk::Button::with_label("生成认证链接");
    let finish_button = gtk::Button::with_label("提交回调");
    finish_button.add_css_class("suggested-action");
    actions.append(&start_button);
    actions.append(&finish_button);
    content.append(&actions);

    let panel = AuthPanel {
        account_id: account.id.clone(),
        window: dialog.clone(),
        status_label: status_label.clone(),
        auth_url_entry: auth_url_entry.clone(),
    };
    state.auth_panel.replace(Some(panel));

    let start_state = Rc::clone(&state);
    let start_account = account.clone();
    let start_auth_url_entry = auth_url_entry.clone();
    let start_auth_response_entry = auth_response_entry.clone();
    let start_status_label = status_label.clone();
    start_button.connect_clicked(move |_| {
        if !Path::new(&start_account.config_dir).exists()
            && let Err(error) = fs::create_dir_all(&start_account.config_dir)
        {
            show_toast(&start_state, &format!("无法创建配置目录: {error}"));
            return;
        }
        if !begin_active_operation(
            &start_state,
            &start_account.id,
            ActiveOperation::Authentication,
        ) {
            return;
        }

        update_account_status(
            &start_state,
            &start_account.id,
            AccountStatus::Authenticating,
        );
        start_auth_url_entry.set_text("");
        start_auth_response_entry.set_text("");
        start_status_label.set_label("正在生成认证链接");
        start_authentication(
            start_account.clone(),
            onedrive_command(&start_state),
            start_state.sender.clone(),
        );
    });

    let finish_state = Rc::clone(&state);
    let finish_account = account.clone();
    let finish_auth_response_entry = auth_response_entry.clone();
    finish_button.connect_clicked(move |_| {
        let response = finish_auth_response_entry.text().trim().to_string();
        if response.is_empty() {
            show_toast(&finish_state, "请粘贴 redirect URI");
            return;
        }
        let response_file = auth_response_path(&finish_account);
        match fs::write(response_file, response) {
            Ok(()) => show_toast(&finish_state, "已提交认证回调"),
            Err(error) => show_toast(&finish_state, &format!("写入认证回调失败: {error}")),
        }
    });

    let close_state = Rc::clone(&state);
    let close_account_id = account.id.clone();
    close_button.connect_clicked(move |_| {
        close_auth_panel(&close_state, &close_account_id);
    });

    let request_close_state = Rc::clone(&state);
    let request_close_account_id = account.id.clone();
    dialog.connect_close_request(move |_| {
        close_auth_panel(&request_close_state, &request_close_account_id);
        glib::Propagation::Stop
    });

    dialog.set_content(Some(&root));
    dialog.present();
}

fn close_auth_panel(state: &AppState, account_id: &str) {
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

fn show_add_account_dialog(state: Rc<AppState>) {
    let dialog = adw::Window::builder()
        .title("添加账号")
        .transient_for(&state.window)
        .modal(true)
        .default_width(520)
        .build();

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let header = adw::HeaderBar::new();
    let cancel_button = gtk::Button::with_label("取消");
    let add_button = gtk::Button::with_label("添加并认证");
    add_button.add_css_class("suggested-action");
    header.pack_start(&cancel_button);
    header.pack_end(&add_button);
    root.append(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&content);

    let name_entry = gtk::Entry::builder()
        .placeholder_text("Profile 名称，例如 Personal")
        .text(default_account_name())
        .build();
    let email_entry = gtk::Entry::builder()
        .placeholder_text("账号标识，例如 john@example.com")
        .build();
    let sync_dir_entry = gtk::Entry::builder()
        .placeholder_text("本地同步目录")
        .text(default_sync_dir())
        .build();

    content.append(&form_row("名称", &name_entry));
    content.append(&form_row("账号", &email_entry));
    content.append(&form_row("同步目录", &sync_dir_entry));

    let cancel_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        cancel_dialog.close();
    });

    let dialog_state = Rc::clone(&state);
    let add_dialog = dialog.clone();
    add_button.connect_clicked(move |_| {
        let name = name_entry.text().trim().to_string();
        let email = email_entry.text().trim().to_string();
        let sync_dir = sync_dir_entry.text().trim().to_string();
        match create_account(&dialog_state.accounts.borrow(), &name, &email, &sync_dir) {
            Ok(account) => {
                let auth_account = account.clone();
                dialog_state.accounts.borrow_mut().push(account);
                if let Err(error) = save_accounts(&dialog_state.accounts.borrow()) {
                    show_toast(&dialog_state, &format!("保存账号失败: {error}"));
                }
                let last_index = dialog_state.accounts.borrow().len().saturating_sub(1);
                dialog_state.selected_index.set(last_index);
                rebuild_profile_list(&dialog_state);
                refresh_content(&dialog_state);
                dialog_state.transfers.clear();
                show_toast(&dialog_state, "账号已添加");
                add_dialog.close();
                show_auth_dialog(Rc::clone(&dialog_state), auth_account);
            }
            Err(error) => show_toast(&dialog_state, &format!("添加账号失败: {error}")),
        }
    });

    dialog.set_content(Some(&root));
    dialog.present();
}

fn show_edit_profile_dialog(state: Rc<AppState>, account: Account) {
    let dialog = adw::Window::builder()
        .title("编辑 Profile")
        .transient_for(&state.window)
        .modal(true)
        .default_width(560)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let close_button = gtk::Button::with_label("关闭");
    let save_button = gtk::Button::with_label("保存");
    save_button.add_css_class("suggested-action");
    header.pack_start(&close_button);
    header.pack_end(&save_button);
    root.append(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&content);

    let name_entry = gtk::Entry::builder().text(&account.name).build();
    let email_entry = gtk::Entry::builder()
        .text(account_label(&account))
        .editable(false)
        .build();
    let sync_dir_entry = gtk::Entry::builder()
        .text(&account.sync_dir)
        .editable(false)
        .build();
    let status_entry = gtk::Entry::builder()
        .text(status_label(&account.status))
        .editable(false)
        .build();
    content.append(&form_row("Profile 名称", &name_entry));
    content.append(&form_row("账号标识", &email_entry));
    content.append(&form_row("同步目录", &sync_dir_entry));
    content.append(&form_row("认证状态", &status_entry));

    let danger_label = gtk::Label::builder()
        .label("危险操作")
        .halign(Align::Start)
        .css_classes(["heading"])
        .margin_top(10)
        .build();
    content.append(&danger_label);

    let danger_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let logout_button = gtk::Button::with_label("退出登录");
    let remove_button = gtk::Button::with_label("移除 Profile");
    logout_button.add_css_class("destructive-action");
    remove_button.add_css_class("destructive-action");
    let can_mutate = can_mutate_profile(&state, &account);
    save_button.set_sensitive(can_mutate);
    logout_button.set_sensitive(can_mutate && state.client_check.borrow().is_ready());
    remove_button.set_sensitive(can_mutate);
    danger_box.append(&logout_button);
    danger_box.append(&remove_button);
    content.append(&danger_box);

    let close_dialog = dialog.clone();
    close_button.connect_clicked(move |_| close_dialog.close());

    let save_state = Rc::clone(&state);
    let save_dialog = dialog.clone();
    let save_account_id = account.id.clone();
    save_button.connect_clicked(move |_| {
        let next = name_entry.text().trim().to_string();
        if next.is_empty() {
            show_toast(&save_state, "名称不能为空");
            return;
        }
        if save_state
            .accounts
            .borrow()
            .iter()
            .any(|stored| stored.id != save_account_id && stored.name == next)
        {
            show_toast(&save_state, "Profile 名称已存在");
            return;
        }
        if let Some(stored) = save_state
            .accounts
            .borrow_mut()
            .iter_mut()
            .find(|stored| stored.id == save_account_id)
        {
            stored.name = next;
        }
        if let Err(error) = save_accounts(&save_state.accounts.borrow()) {
            show_toast(&save_state, &format!("保存账号失败: {error}"));
            return;
        }
        rebuild_profile_list(&save_state);
        refresh_content(&save_state);
        save_dialog.close();
        show_toast(&save_state, "Profile 已保存");
    });

    let logout_state = Rc::clone(&state);
    let logout_account = account.clone();
    logout_button.connect_clicked(move |_| {
        confirm_logout_profile(Rc::clone(&logout_state), logout_account.clone());
    });

    let remove_state = Rc::clone(&state);
    let remove_account = account.clone();
    remove_button.connect_clicked(move |_| {
        confirm_remove_profile(Rc::clone(&remove_state), remove_account.clone());
    });

    dialog.set_content(Some(&root));
    dialog.present();
}

fn confirm_remove_profile(state: Rc<AppState>, account: Account) {
    if !can_mutate_profile(&state, &account) {
        show_toast(&state, "请先停止该 Profile 的认证、同步或持续同步");
        return;
    }
    show_remove_confirmation(Rc::clone(&state), account, move |state, account| {
        let selected = state.selected_index.get();
        state
            .accounts
            .borrow_mut()
            .retain(|stored| stored.id != account.id);
        state.selected_index.set(selected.saturating_sub(1));
        if let Err(error) = save_accounts(&state.accounts.borrow()) {
            show_toast(&state, &format!("保存账号失败: {error}"));
        }
        rebuild_profile_list(&state);
        refresh_content(&state);
        state.transfers.clear();
    });
}

fn confirm_logout_profile(state: Rc<AppState>, account: Account) {
    if !can_mutate_profile(&state, &account) {
        show_toast(&state, "请先停止该 Profile 的认证、同步或持续同步");
        return;
    }
    if !ensure_client_ready(&state) {
        return;
    }
    show_confirmation(
        Rc::clone(&state),
        "退出登录",
        "将运行 onedrive --logout。不会删除 OneDrive 云端文件，也不会删除本地同步目录。",
        "退出登录",
        move |state| {
            if !begin_active_operation(&state, &account.id, ActiveOperation::Logout) {
                return;
            }
            start_logout(
                account.clone(),
                onedrive_command(&state),
                state.sender.clone(),
            );
        },
    );
}

fn show_remove_confirmation<F>(state: Rc<AppState>, account: Account, on_confirm: F)
where
    F: Fn(Rc<AppState>, Account) + 'static,
{
    let dialog = adw::Window::builder()
        .title("移除 Profile")
        .transient_for(&state.window)
        .modal(true)
        .default_width(540)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    let cancel_button = gtk::Button::with_label("取消");
    let confirm_button = gtk::Button::with_label("移除 Profile");
    confirm_button.add_css_class("destructive-action");
    confirm_button.set_sensitive(false);
    header.pack_start(&cancel_button);
    header.pack_end(&confirm_button);
    root.append(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&content);
    content.append(
        &gtk::Label::builder()
            .label("这只会从 OneSync 列表移除该 Profile。不会删除 OneDrive 云端文件，也不会删除本地同步目录。")
            .wrap(true)
            .halign(Align::Start)
            .build(),
    );
    let name_entry = gtk::Entry::builder()
        .placeholder_text("输入当前 Profile 名称以确认")
        .build();
    content.append(&form_row("确认名称", &name_entry));

    let expected_name = account.name.clone();
    let confirm_for_entry = confirm_button.clone();
    name_entry.connect_changed(move |entry| {
        confirm_for_entry.set_sensitive(remove_confirmation_matches(&expected_name, &entry.text()));
    });

    let cancel_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| cancel_dialog.close());
    let confirm_dialog = dialog.clone();
    confirm_button.connect_clicked(move |_| {
        on_confirm(Rc::clone(&state), account.clone());
        confirm_dialog.close();
    });
    dialog.set_content(Some(&root));
    dialog.present();
}

fn show_confirmation<F>(
    state: Rc<AppState>,
    title: &str,
    message: &str,
    confirm_label: &str,
    on_confirm: F,
) where
    F: Fn(Rc<AppState>) + 'static,
{
    let dialog = adw::Window::builder()
        .title(title)
        .transient_for(&state.window)
        .modal(true)
        .default_width(500)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    let cancel_button = gtk::Button::with_label("取消");
    let confirm_button = gtk::Button::with_label(confirm_label);
    confirm_button.add_css_class("destructive-action");
    header.pack_start(&cancel_button);
    header.pack_end(&confirm_button);
    root.append(&header);
    let label = gtk::Label::builder()
        .label(message)
        .wrap(true)
        .halign(Align::Start)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&label);

    let cancel_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| cancel_dialog.close());
    let confirm_dialog = dialog.clone();
    confirm_button.connect_clicked(move |_| {
        on_confirm(Rc::clone(&state));
        confirm_dialog.close();
    });
    dialog.set_content(Some(&root));
    dialog.present();
}

fn show_warning_window(state: &AppState, title: &str, message: &str) {
    let dialog = adw::Window::builder()
        .title(title)
        .transient_for(&state.window)
        .modal(true)
        .default_width(560)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    let close_button = gtk::Button::with_label("知道了");
    header.pack_end(&close_button);
    root.append(&header);
    let label = gtk::Label::builder()
        .label(message)
        .wrap(true)
        .halign(Align::Start)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&label);
    let close_dialog = dialog.clone();
    close_button.connect_clicked(move |_| close_dialog.close());
    dialog.set_content(Some(&root));
    dialog.present();
}

fn ensure_client_ready(state: &AppState) -> bool {
    let check = state.client_check.borrow();
    if check.is_ready() {
        true
    } else {
        show_toast(state, &check.message());
        false
    }
}

fn rebuild_profile_list(state: &AppState) {
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

fn build_profile_row(account: &Account) -> gtk::ListBoxRow {
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

fn build_account_actions_popover(
    items: &[(&gtk::Button, &str, &str)],
) -> gtk::Popover {
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .width_request(ACCOUNT_CONTEXT_MENU_WIDTH)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();

    for &(button, icon_name, label) in items {
        set_menu_button_content(button, icon_name, label);
        button.add_css_class("flat");
        button.set_halign(Align::Fill);
        content.append(button);
    }

    popover.set_child(Some(&content));
    popover
}

fn build_profile_context_popover() -> gtk::Popover {
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();

    for (index, (icon_name, label)) in [
        ("view-refresh-symbolic", "同步一次"),
        ("media-playback-start-symbolic", "开始持续同步"),
        ("folder-open-symbolic", "打开同步目录"),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        let item = gtk::Button::builder()
            .halign(Align::Fill)
            .sensitive(false)
            .css_classes(["flat"])
            .build();
        set_menu_button_content(&item, icon_name, label);
        content.append(&item);
    }

    popover.set_child(Some(&content));
    popover
}

fn set_menu_button_content(button: &gtk::Button, icon_name: &str, label: &str) {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(Align::Fill)
        .hexpand(true)
        .build();
    row.append(&gtk::Image::from_icon_name(icon_name));
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(Align::Start)
            .hexpand(true)
            .build(),
    );
    button.set_child(Some(&row));
}

fn refresh_content(state: &AppState) {
    let Some(account) = selected_account(state) else {
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

fn selected_account(state: &AppState) -> Option<Account> {
    state
        .accounts
        .borrow()
        .get(state.selected_index.get())
        .cloned()
}

fn update_account_status(state: &AppState, account_id: &str, status: AccountStatus) {
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

fn status_title(status: &AccountStatus) -> &'static str {
    match status {
        AccountStatus::NeedsAuth => "需要认证",
        AccountStatus::Authenticating => "认证中",
        AccountStatus::Authenticated => "已认证",
        AccountStatus::Syncing => "同步中",
        AccountStatus::Monitoring => "持续同步中",
        AccountStatus::Error(_) => "需要处理",
    }
}

fn status_label(status: &AccountStatus) -> &str {
    match status {
        AccountStatus::NeedsAuth => "未认证",
        AccountStatus::Authenticating => "认证中",
        AccountStatus::Authenticated => "已认证",
        AccountStatus::Syncing => "同步中",
        AccountStatus::Monitoring => "持续同步中",
        AccountStatus::Error(message) => message.as_str(),
    }
}

fn status_detail(account: &Account) -> String {
    match &account.status {
        AccountStatus::NeedsAuth => format!("配置目录: {}", account.config_dir),
        AccountStatus::Authenticating => "打开认证链接，登录后粘贴 redirect URI".to_string(),
        AccountStatus::Authenticated => format!("同步目录: {}", account.sync_dir),
        AccountStatus::Syncing => "onedrive CLI 正在执行一次同步".to_string(),
        AccountStatus::Monitoring => "onedrive CLI 正在持续监听本地和远端变化".to_string(),
        AccountStatus::Error(message) => format!("最近错误: {message}"),
    }
}

fn show_toast(state: &AppState, message: &str) {
    state.toast_overlay.add_toast(adw::Toast::new(message));
}

fn onedrive_command(state: &AppState) -> String {
    state.onedrive_command.clone()
}

fn account_label(account: &Account) -> String {
    if account.email.trim().is_empty() {
        account.id.clone()
    } else {
        account.email.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_profile_requires_exact_current_name() {
        assert!(remove_confirmation_matches("Work Drive", "Work Drive"));
        assert!(!remove_confirmation_matches("Work Drive", "work drive"));
        assert!(!remove_confirmation_matches("Work Drive", " Work Drive "));
        assert!(!remove_confirmation_matches("Work Drive", "Personal"));
    }
}
