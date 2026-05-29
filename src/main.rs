use adw::prelude::*;
use gtk::{Align, glib};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    env, fs,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const APP_ID: &str = "io.github.onesync.Demo";
const FILE_COLUMN_WIDTH: i32 = 220;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum AccountStatus {
    NeedsAuth,
    Authenticating,
    Authenticated,
    Syncing,
    Monitoring,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Account {
    id: String,
    name: String,
    email: String,
    config_dir: String,
    sync_dir: String,
    status: AccountStatus,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AccountStore {
    accounts: Vec<Account>,
}

#[derive(Debug, Clone)]
struct SyncFile {
    name: String,
    state: String,
    progress: f64,
    icon: &'static str,
}

#[derive(Debug)]
enum BackendEvent {
    AuthUrl {
        account_id: String,
        url: String,
    },
    AuthFinished {
        account_id: String,
        success: bool,
    },
    SyncFinished {
        account_id: String,
        success: bool,
    },
    TransferEvent {
        account_id: String,
        file: SyncFile,
    },
    MonitorStopped {
        account_id: String,
        success: bool,
        requested_stop: bool,
    },
}

struct AppState {
    accounts: RefCell<Vec<Account>>,
    selected_index: Cell<usize>,
    sender: mpsc::Sender<BackendEvent>,
    receiver: RefCell<mpsc::Receiver<BackendEvent>>,
    auth_panel: RefCell<Option<AuthPanel>>,
    monitors: RefCell<HashMap<String, MonitorHandle>>,
    toast_overlay: adw::ToastOverlay,
    window: adw::ApplicationWindow,
    profile_list: gtk::ListBox,
    title: adw::WindowTitle,
    status_title: gtk::Label,
    status_detail: gtk::Label,
    files_list: gtk::ListBox,
    transfer_rows: RefCell<HashMap<String, TransferRow>>,
    reauth_button: gtk::Button,
    one_time_sync_button: gtk::Button,
    monitor_button: gtk::Button,
}

#[derive(Clone)]
struct AuthPanel {
    account_id: String,
    window: adw::Window,
    status_label: gtk::Label,
    auth_url_entry: gtk::Entry,
}

#[derive(Clone)]
struct MonitorHandle {
    child: Arc<Mutex<Child>>,
    stop_requested: Arc<AtomicBool>,
}

#[derive(Clone)]
struct TransferRow {
    state_label: gtk::Label,
    progress_bar: gtk::ProgressBar,
}

fn main() -> glib::ExitCode {
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
        sender,
        receiver: RefCell::new(receiver),
        auth_panel: RefCell::new(None),
        monitors: RefCell::new(HashMap::new()),
        toast_overlay: toast_overlay.clone(),
        window: window.clone(),
        profile_list,
        title: content_widgets.title,
        status_title: content_widgets.status_title,
        status_detail: content_widgets.status_detail,
        files_list: content_widgets.files_list,
        transfer_rows: RefCell::new(HashMap::new()),
        reauth_button: content_widgets.reauth_button,
        one_time_sync_button: content_widgets.one_time_sync_button,
        monitor_button: content_widgets.monitor_button,
    });

    let sidebar = build_sidebar(Rc::clone(&state));
    connect_actions(Rc::clone(&state));
    connect_shutdown(Rc::clone(&state));
    refresh_accounts_from_disk(&state);
    rebuild_profile_list(&state);
    refresh_content(&state);
    install_backend_event_pump(Rc::clone(&state));

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
    reauth_button: gtk::Button,
    one_time_sync_button: gtk::Button,
    monitor_button: gtk::Button,
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
            clear_transfer_rows(&select_state);
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
    let reauth_button = command_button("avatar-default-symbolic", "重新认证");
    let one_time_sync_button = command_button("view-refresh-symbolic", "一次同步");
    let monitor_button = command_button("media-playback-start-symbolic", "持续同步");
    actions.append(&reauth_button);
    actions.append(&one_time_sync_button);
    actions.append(&monitor_button);

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
            reauth_button,
            one_time_sync_button,
            monitor_button,
        },
    )
}

fn connect_actions(state: Rc<AppState>) {
    let reauth_state = Rc::clone(&state);
    state.reauth_button.connect_clicked(move |_| {
        let Some(account) = selected_account(&reauth_state) else {
            show_toast(&reauth_state, "请先选择账号");
            return;
        };
        show_auth_dialog(Rc::clone(&reauth_state), account);
    });

    let one_time_sync_state = Rc::clone(&state);
    state.one_time_sync_button.connect_clicked(move |_| {
        let Some(account) = selected_account(&one_time_sync_state) else {
            show_toast(&one_time_sync_state, "请先选择账号");
            return;
        };
        if !is_authenticated(&account) {
            show_toast(&one_time_sync_state, "账号尚未完成认证");
            return;
        }
        if is_monitor_running(&one_time_sync_state, &account.id) {
            show_toast(&one_time_sync_state, "持续同步运行中，不能同时执行一次同步");
            return;
        }
        update_account_status(&one_time_sync_state, &account.id, AccountStatus::Syncing);
        clear_transfer_rows(&one_time_sync_state);
        refresh_content(&one_time_sync_state);
        start_sync(account, one_time_sync_state.sender.clone());
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
        if matches!(account.status, AccountStatus::Syncing) {
            show_toast(&monitor_state, "一次同步运行中，请稍后再启动持续同步");
            return;
        }

        match start_monitor(account.clone(), monitor_state.sender.clone()) {
            Ok(handle) => {
                monitor_state
                    .monitors
                    .borrow_mut()
                    .insert(account.id.clone(), handle);
                update_account_status(&monitor_state, &account.id, AccountStatus::Monitoring);
                clear_transfer_rows(&monitor_state);
                refresh_content(&monitor_state);
            }
            Err(error) => show_toast(&monitor_state, &format!("启动持续同步失败: {error}")),
        }
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

fn drain_backend_events(state: &AppState) {
    let mut events = VecDeque::new();
    {
        let receiver = state.receiver.borrow();
        while let Ok(event) = receiver.try_recv() {
            events.push_back(event);
        }
    }

    while let Some(event) = events.pop_front() {
        match event {
            BackendEvent::AuthUrl { account_id, url } => {
                if let Some(panel) = state.auth_panel.borrow().as_ref() {
                    if panel.account_id == account_id {
                        panel.auth_url_entry.set_text(&url);
                        panel
                            .status_label
                            .set_label("认证链接已生成，请复制到浏览器登录");
                    }
                }
                if selected_account(state).is_some_and(|account| account.id == account_id) {
                    show_toast(state, "认证链接已生成");
                }
            }
            BackendEvent::AuthFinished {
                account_id,
                success,
            } => {
                let status = if success {
                    AccountStatus::Authenticated
                } else {
                    AccountStatus::Error("认证失败".to_string())
                };
                update_account_status(state, &account_id, status);
                clear_transfer_rows(state);
                if let Some(panel) = state.auth_panel.borrow().as_ref() {
                    if panel.account_id == account_id {
                        panel.status_label.set_label(if success {
                            "认证完成，可以关闭窗口"
                        } else {
                            "认证失败，请检查输出后重试"
                        });
                    }
                }
                show_toast(
                    state,
                    if success {
                        "认证完成"
                    } else {
                        "认证失败"
                    },
                );
            }
            BackendEvent::SyncFinished {
                account_id,
                success,
            } => {
                let status = if success {
                    AccountStatus::Authenticated
                } else {
                    AccountStatus::Error("同步失败".to_string())
                };
                update_account_status(state, &account_id, status);
                show_toast(
                    state,
                    if success {
                        "同步完成"
                    } else {
                        "同步失败"
                    },
                );
            }
            BackendEvent::TransferEvent { account_id, file } => {
                if selected_account(state).is_some_and(|account| account.id == account_id) {
                    upsert_transfer_row(state, file);
                }
            }
            BackendEvent::MonitorStopped {
                account_id,
                success,
                requested_stop,
            } => {
                state.monitors.borrow_mut().remove(&account_id);
                let status = if success || requested_stop {
                    AccountStatus::Authenticated
                } else {
                    AccountStatus::Error("持续同步停止".to_string())
                };
                update_account_status(state, &account_id, status);
                show_toast(
                    state,
                    if requested_stop {
                        "持续同步已停止"
                    } else if success {
                        "持续同步已结束"
                    } else {
                        "持续同步异常停止"
                    },
                );
            }
        }
    }
}

fn start_authentication(account: Account, sender: mpsc::Sender<BackendEvent>) {
    thread::spawn(move || {
        let auth_url = auth_url_path(&account);
        let auth_response = auth_response_path(&account);
        let _ = fs::remove_file(&auth_url);
        let _ = fs::remove_file(&auth_response);

        let child = match Command::new("onedrive")
            .arg("--confdir")
            .arg(&account.config_dir)
            .arg("--auth-files")
            .arg(format!(
                "{}:{}",
                auth_url.display(),
                auth_response.display()
            ))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id,
                    success: false,
                });
                eprintln!("failed to start onedrive authentication: {error}");
                return;
            }
        };

        for _ in 0..300 {
            if let Ok(url) = fs::read_to_string(&auth_url) {
                let trimmed = url.trim();
                if !trimmed.is_empty() {
                    let _ = sender.send(BackendEvent::AuthUrl {
                        account_id: account.id.clone(),
                        url: trimmed.to_string(),
                    });
                    break;
                }
            }
            thread::sleep(Duration::from_millis(200));
        }

        match child.wait_with_output() {
            Ok(output) => {
                let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
                combined.push_str(&String::from_utf8_lossy(&output.stderr));
                let success = output.status.success() || is_authenticated(&account);
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id,
                    success,
                });
                if !combined.trim().is_empty() {
                    eprintln!("{combined}");
                }
            }
            Err(error) => {
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id,
                    success: false,
                });
                eprintln!("failed to wait for onedrive authentication: {error}");
            }
        }
    });
}

fn start_sync(account: Account, sender: mpsc::Sender<BackendEvent>) {
    thread::spawn(move || {
        if let Err(error) = ensure_transfer_metrics_enabled(&account) {
            eprintln!("failed to enable onedrive transfer metrics: {error}");
        }

        let mut child = match Command::new("onedrive")
            .arg("--confdir")
            .arg(&account.config_dir)
            .arg("--sync")
            .arg("--verbose")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = sender.send(BackendEvent::SyncFinished {
                    account_id: account.id,
                    success: false,
                });
                eprintln!("failed to start onedrive sync: {error}");
                return;
            }
        };

        if let Some(stdout) = child.stdout.take() {
            spawn_transfer_reader(account.id.clone(), stdout, sender.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_transfer_reader(account.id.clone(), stderr, sender.clone());
        }

        match child.wait() {
            Ok(status) => {
                let _ = sender.send(BackendEvent::SyncFinished {
                    account_id: account.id,
                    success: status.success(),
                });
            }
            Err(error) => {
                let _ = sender.send(BackendEvent::SyncFinished {
                    account_id: account.id,
                    success: false,
                });
                eprintln!("failed to wait for onedrive sync: {error}");
            }
        }
    });
}

fn start_monitor(
    account: Account,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<MonitorHandle> {
    if let Err(error) = ensure_transfer_metrics_enabled(&account) {
        eprintln!("failed to enable onedrive transfer metrics: {error}");
    }

    let mut child = Command::new("onedrive")
        .arg("--confdir")
        .arg(&account.config_dir)
        .arg("--monitor")
        .arg("--verbose")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdout) = child.stdout.take() {
        spawn_transfer_reader(account.id.clone(), stdout, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_transfer_reader(account.id.clone(), stderr, sender.clone());
    }

    let child = Arc::new(Mutex::new(child));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let wait_child = Arc::clone(&child);
    let wait_stop_requested = Arc::clone(&stop_requested);

    thread::spawn(move || {
        let success = loop {
            let result = {
                match wait_child.lock() {
                    Ok(mut child) => child.try_wait(),
                    Err(_) => {
                        let _ = sender.send(BackendEvent::MonitorStopped {
                            account_id: account.id,
                            success: false,
                            requested_stop: wait_stop_requested.load(Ordering::SeqCst),
                        });
                        eprintln!("failed to lock monitor process");
                        return;
                    }
                }
            };

            match result {
                Ok(Some(status)) => break status.success(),
                Ok(None) => thread::sleep(Duration::from_millis(500)),
                Err(error) => {
                    let _ = sender.send(BackendEvent::MonitorStopped {
                        account_id: account.id,
                        success: false,
                        requested_stop: wait_stop_requested.load(Ordering::SeqCst),
                    });
                    eprintln!("failed to poll monitor process: {error}");
                    return;
                }
            }
        };

        let _ = sender.send(BackendEvent::MonitorStopped {
            account_id: account.id,
            success,
            requested_stop: wait_stop_requested.load(Ordering::SeqCst),
        });
    });

    Ok(MonitorHandle {
        child,
        stop_requested,
    })
}

fn spawn_transfer_reader<R>(account_id: String, reader: R, sender: mpsc::Sender<BackendEvent>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    if byte[0] == b'\n' || byte[0] == b'\r' {
                        send_transfer_chunk(&account_id, &buffer, &sender);
                        buffer.clear();
                    } else {
                        buffer.push(byte[0]);
                    }
                }
                Err(error) => {
                    eprintln!("failed to read onedrive output: {error}");
                    break;
                }
            }
        }
        if !buffer.is_empty() {
            send_transfer_chunk(&account_id, &buffer, &sender);
        }
    });
}

fn send_transfer_chunk(account_id: &str, chunk: &[u8], sender: &mpsc::Sender<BackendEvent>) {
    let line = String::from_utf8_lossy(chunk);
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Some(file) = parse_transfer_line(line) {
        let _ = sender.send(BackendEvent::TransferEvent {
            account_id: account_id.to_string(),
            file,
        });
    }
}

fn stop_monitor(state: &AppState, account_id: &str) {
    let Some(handle) = state.monitors.borrow().get(account_id).cloned() else {
        show_toast(state, "持续同步未运行");
        return;
    };

    handle.stop_requested.store(true, Ordering::SeqCst);
    match handle.child.lock() {
        Ok(mut child) => match child.kill() {
            Ok(()) => show_toast(state, "正在停止持续同步"),
            Err(error) => show_toast(state, &format!("停止持续同步失败: {error}")),
        },
        Err(_) => show_toast(state, "无法访问持续同步进程"),
    }
}

fn stop_all_monitors(state: &AppState) {
    let handles: Vec<MonitorHandle> = state.monitors.borrow().values().cloned().collect();
    for handle in handles {
        handle.stop_requested.store(true, Ordering::SeqCst);
        if let Ok(mut child) = handle.child.lock() {
            let _ = child.kill();
        }
    }
}

fn is_monitor_running(state: &AppState, account_id: &str) -> bool {
    state.monitors.borrow().contains_key(account_id)
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
        if !Path::new(&start_account.config_dir).exists() {
            if let Err(error) = fs::create_dir_all(&start_account.config_dir) {
                show_toast(&start_state, &format!("无法创建配置目录: {error}"));
                return;
            }
        }

        update_account_status(
            &start_state,
            &start_account.id,
            AccountStatus::Authenticating,
        );
        start_auth_url_entry.set_text("");
        start_auth_response_entry.set_text("");
        start_status_label.set_label("正在生成认证链接");
        start_authentication(start_account.clone(), start_state.sender.clone());
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
        match create_account(&name, &email, &sync_dir) {
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
                clear_transfer_rows(&dialog_state);
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

fn form_row(label: &str, entry: &gtk::Entry) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let title = gtk::Label::builder()
        .label(label)
        .halign(Align::Start)
        .width_request(90)
        .build();
    row.append(&title);
    row.append(entry);
    entry.set_hexpand(true);
    row
}

fn create_account(name: &str, email: &str, sync_dir: &str) -> io::Result<Account> {
    let name = if name.is_empty() { "OneDrive" } else { name };
    let id = format!("{}-{}", sanitize_id(name), unix_timestamp());
    let config_dir = profiles_root().join(&id);
    fs::create_dir_all(&config_dir)?;
    let config_file = config_dir.join("config");
    fs::write(
        config_file,
        format!(
            "# Managed by OneSync\nsync_dir = \"{}\"\n",
            sync_dir.replace('\\', "\\\\").replace('"', "\\\"")
        ),
    )?;
    fs::create_dir_all(expand_home(sync_dir))?;

    Ok(Account {
        id,
        name: name.to_string(),
        email: email.to_string(),
        config_dir: config_dir.to_string_lossy().to_string(),
        sync_dir: sync_dir.to_string(),
        status: AccountStatus::NeedsAuth,
    })
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
    row
}

fn refresh_content(state: &AppState) {
    let Some(account) = selected_account(state) else {
        state.title.set_title("OneSync");
        state.title.set_subtitle("未选择账号");
        state.status_title.set_label("未配置");
        state.status_detail.set_label("添加账号后开始认证");
        clear_transfer_rows(state);
        return;
    };

    state.title.set_title(&account.name);
    state.title.set_subtitle(&account_label(&account));
    state.status_title.set_label(status_title(&account.status));
    state.status_detail.set_label(&status_detail(&account));
    state
        .one_time_sync_button
        .set_sensitive(matches!(account.status, AccountStatus::Authenticated));
    state.reauth_button.set_sensitive(!matches!(
        account.status,
        AccountStatus::Authenticating | AccountStatus::Syncing | AccountStatus::Monitoring
    ));
    state.monitor_button.set_sensitive(matches!(
        account.status,
        AccountStatus::Authenticated | AccountStatus::Monitoring
    ));
    if matches!(account.status, AccountStatus::Monitoring) {
        set_command_button_content(
            &state.monitor_button,
            "media-playback-stop-symbolic",
            "停止持续同步",
        );
    } else {
        set_command_button_content(
            &state.monitor_button,
            "media-playback-start-symbolic",
            "持续同步",
        );
    }
}

fn clear_transfer_rows(state: &AppState) {
    while let Some(child) = state.files_list.first_child() {
        state.files_list.remove(&child);
    }
    state.transfer_rows.borrow_mut().clear();
}

fn upsert_transfer_row(state: &AppState, file: SyncFile) {
    if let Some(row) = state.transfer_rows.borrow().get(&file.name).cloned() {
        row.state_label.set_label(&file.state);
        row.progress_bar.set_fraction(file.progress);
        return;
    }

    let key = file.name.clone();
    let (row, transfer_row) = build_file_row(file);
    state.files_list.prepend(&row);
    state.transfer_rows.borrow_mut().insert(key, transfer_row);
}

#[derive(Clone, Copy)]
struct TransferPattern {
    prefix: &'static str,
    action: &'static str,
    icon: &'static str,
    complete_without_done: bool,
}

const TRANSFER_PATTERNS: &[TransferPattern] = &[
    TransferPattern {
        prefix: "Downloading file:",
        action: "下载",
        icon: "go-down-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Downloading file",
        action: "下载",
        icon: "go-down-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Downloading:",
        action: "下载",
        icon: "go-down-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Uploading:",
        action: "上传",
        icon: "go-up-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Uploading modified file:",
        action: "更新",
        icon: "document-save-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Uploading modified file",
        action: "更新",
        icon: "document-save-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Uploading new file:",
        action: "上传",
        icon: "go-up-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Uploading new file",
        action: "上传",
        icon: "go-up-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Uploading file:",
        action: "上传",
        icon: "go-up-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Uploading file",
        action: "上传",
        icon: "go-up-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Deleting item from Microsoft OneDrive:",
        action: "删除",
        icon: "edit-delete-symbolic",
        complete_without_done: true,
    },
    TransferPattern {
        prefix: "Deleting local file:",
        action: "删除",
        icon: "edit-delete-symbolic",
        complete_without_done: true,
    },
    TransferPattern {
        prefix: "Deleting remote file:",
        action: "删除",
        icon: "edit-delete-symbolic",
        complete_without_done: true,
    },
    TransferPattern {
        prefix: "Deleting file:",
        action: "删除",
        icon: "edit-delete-symbolic",
        complete_without_done: true,
    },
    TransferPattern {
        prefix: "Deleting local item:",
        action: "删除",
        icon: "edit-delete-symbolic",
        complete_without_done: true,
    },
    TransferPattern {
        prefix: "Deleting remote item:",
        action: "删除",
        icon: "edit-delete-symbolic",
        complete_without_done: true,
    },
    TransferPattern {
        prefix: "Deleting item:",
        action: "删除",
        icon: "edit-delete-symbolic",
        complete_without_done: true,
    },
    TransferPattern {
        prefix: "Moving file:",
        action: "移动",
        icon: "go-jump-symbolic",
        complete_without_done: false,
    },
    TransferPattern {
        prefix: "Renaming file:",
        action: "重命名",
        icon: "document-edit-symbolic",
        complete_without_done: false,
    },
];

fn parse_transfer_line(line: &str) -> Option<SyncFile> {
    let trimmed = line.trim();
    let pattern = TRANSFER_PATTERNS
        .iter()
        .find(|pattern| trimmed.starts_with(pattern.prefix))?;

    let mut path = trimmed
        .trim_start_matches(pattern.prefix)
        .trim_start_matches(':')
        .trim();
    let failed = path.ends_with("failed!") || path.contains(" ... failed");
    let done = !failed
        && (pattern.complete_without_done
            || path.ends_with("done.")
            || path.ends_with("done")
            || path.contains(" ... done"));

    if let Some((file_path, _)) = path.split_once(" ... ") {
        path = file_path.trim();
    }
    let progress = parse_percent(trimmed).unwrap_or(if failed {
        0.0
    } else if done {
        1.0
    } else {
        0.5
    });

    let state = if failed {
        format!("{}失败", pattern.action)
    } else if done || progress >= 1.0 {
        format!("{}完成", pattern.action)
    } else {
        format!("正在{}", pattern.action)
    };

    Some(SyncFile {
        name: path.to_string(),
        state,
        progress,
        icon: pattern.icon,
    })
}

fn parse_percent(line: &str) -> Option<f64> {
    let percent_index = line.find('%')?;
    let before_percent = &line[..percent_index];
    let digits_start = before_percent
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_ascii_digit())?
        .0
        + 1;
    let percent = before_percent[digits_start..].trim().parse::<f64>().ok()?;
    Some((percent / 100.0).clamp(0.0, 1.0))
}

fn build_file_row(file: SyncFile) -> (gtk::ListBoxRow, TransferRow) {
    let list_row = gtk::ListBoxRow::new();

    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();

    let icon = gtk::Image::from_icon_name(file.icon);
    icon.set_valign(Align::Center);

    let text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    text_box.set_size_request(FILE_COLUMN_WIDTH, -1);

    let name = gtk::Label::builder()
        .label(file.name)
        .halign(Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let state = gtk::Label::builder()
        .label(file.state)
        .halign(Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["dim-label"])
        .build();
    text_box.append(&name);
    text_box.append(&state);

    let progress = gtk::ProgressBar::builder()
        .fraction(file.progress)
        .valign(Align::Center)
        .width_request(FILE_COLUMN_WIDTH)
        .build();
    progress.set_size_request(FILE_COLUMN_WIDTH, -1);
    progress.set_hexpand(false);

    row_box.append(&icon);
    row_box.append(&text_box);
    row_box.append(&progress);
    list_row.set_child(Some(&row_box));

    (
        list_row,
        TransferRow {
            state_label: state,
            progress_bar: progress,
        },
    )
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

fn is_authenticated(account: &Account) -> bool {
    Path::new(&account.config_dir)
        .join("refresh_token")
        .exists()
}

fn ensure_transfer_metrics_enabled(account: &Account) -> io::Result<()> {
    let config_path = Path::new(&account.config_dir).join("config");
    let mut content = fs::read_to_string(&config_path)?;
    let mut found = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if line.trim_start().starts_with("display_transfer_metrics") {
            lines.push("display_transfer_metrics = \"true\"".to_string());
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("display_transfer_metrics = \"true\"\n");
        fs::write(config_path, content)?;
        return Ok(());
    }

    let updated = lines.join("\n") + "\n";
    if updated != content {
        fs::write(config_path, updated)?;
    }
    Ok(())
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

fn command_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    set_command_button_content(&button, icon_name, label);
    button
}

fn set_command_button_content(button: &gtk::Button, icon_name: &str, label: &str) {
    let content = adw::ButtonContent::builder()
        .icon_name(icon_name)
        .label(label)
        .build();

    button.set_child(Some(&content));
}

fn show_toast(state: &AppState, message: &str) {
    state.toast_overlay.add_toast(adw::Toast::new(message));
}

fn account_label(account: &Account) -> String {
    if account.email.trim().is_empty() {
        account.id.clone()
    } else {
        account.email.clone()
    }
}

fn load_store() -> io::Result<AccountStore> {
    let path = store_path();
    if !path.exists() {
        return Ok(AccountStore::default());
    }
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(io::Error::other)
}

fn save_accounts(accounts: &[Account]) -> io::Result<()> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let store = AccountStore {
        accounts: accounts.to_vec(),
    };
    let content = serde_json::to_string_pretty(&store).map_err(io::Error::other)?;
    fs::write(path, content)
}

fn store_path() -> PathBuf {
    config_root().join("accounts.json")
}

fn profiles_root() -> PathBuf {
    config_root().join("profiles")
}

fn config_root() -> PathBuf {
    if let Ok(path) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("onesync");
    }
    home_dir().join(".config").join("onesync")
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn default_account_name() -> String {
    let count = load_store()
        .map(|store| store.accounts.len() + 1)
        .unwrap_or(1);
    format!("OneDrive {count}")
}

fn default_sync_dir() -> String {
    let count = load_store()
        .map(|store| store.accounts.len() + 1)
        .unwrap_or(1);
    if count == 1 {
        "~/OneDrive".to_string()
    } else {
        format!("~/OneDrive-{count}")
    }
}

fn sanitize_id(value: &str) -> String {
    let mut id = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
        } else if character == '-' || character == '_' || character.is_whitespace() {
            id.push('-');
        }
    }
    let trimmed = id.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "onedrive".to_string()
    } else {
        trimmed
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn auth_url_path(account: &Account) -> PathBuf {
    Path::new(&account.config_dir).join("auth-url")
}

fn auth_response_path(account: &Account) -> PathBuf {
    Path::new(&account.config_dir).join("auth-response")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> SyncFile {
        parse_transfer_line(line).expect("line should be parsed as a transfer event")
    }

    fn assert_progress(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_actual_upload_new_file_output() {
        let file = parsed("Uploading new file: ./.onesync-parser-test/move-source.txt ... done");

        assert_eq!(file.name, "./.onesync-parser-test/move-source.txt");
        assert_eq!(file.state, "上传完成");
        assert_eq!(file.icon, "go-up-symbolic");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn parses_actual_upload_progress_output() {
        let file = parsed(
            "Uploading: ./.onesync-progress-test/upload-progress.bin ... 37%  |  ETA    00:00:10",
        );

        assert_eq!(file.name, "./.onesync-progress-test/upload-progress.bin");
        assert_eq!(file.state, "正在上传");
        assert_eq!(file.icon, "go-up-symbolic");
        assert_progress(file.progress, 0.37);
    }

    #[test]
    fn parses_actual_delete_item_output() {
        let file =
            parsed("Deleting item from Microsoft OneDrive: .onesync-parser-test/move-source.txt");

        assert_eq!(file.name, ".onesync-parser-test/move-source.txt");
        assert_eq!(file.state, "删除完成");
        assert_eq!(file.icon, "edit-delete-symbolic");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn parses_actual_modified_file_output() {
        let file = parsed("Uploading modified file: .onesync-parser-test/move-target.txt ... done");

        assert_eq!(file.name, ".onesync-parser-test/move-target.txt");
        assert_eq!(file.state, "更新完成");
        assert_eq!(file.icon, "document-save-symbolic");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn ignores_status_and_scan_output() {
        assert!(parse_transfer_line("Configuration file successfully loaded").is_none());
        assert!(parse_transfer_line("Processing: .onesync-parser-test/move-target.txt").is_none());
        assert!(parse_transfer_line("The file has been deleted locally").is_none());
        assert!(parse_transfer_line("Sync with Microsoft OneDrive is complete").is_none());
    }
}
