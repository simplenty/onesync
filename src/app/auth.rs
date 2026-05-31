use super::{
    begin_active_operation, close_auth_panel, onedrive_command,
    render::show_toast,
    state::{ActiveOperation, AppState, AuthPanel},
    update_account_status,
    widgets::form_row,
};
use crate::{
    account::{Account, AccountStatus, auth_response_path},
    onedrive::start_authentication,
};
use adw::prelude::*;
use gtk::{Align, glib};
use std::{cell::Cell, fs, path::Path, rc::Rc};

pub(in crate::app) fn show_auth_dialog(state: Rc<AppState>, account: Account) {
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
        .label("正在生成认证链接")
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
    let copy_auth_url_button = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("复制认证链接")
        .sensitive(false)
        .build();
    let auth_url_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .hexpand(true)
        .build();
    auth_url_box.append(&auth_url_entry);
    auth_url_box.append(&copy_auth_url_button);
    content.append(&form_widget_row("认证链接", &auth_url_box));
    let copy_button_for_url = copy_auth_url_button.clone();
    let copy_close_blocked = Rc::new(Cell::new(false));
    let copy_close_blocked_for_url = Rc::clone(&copy_close_blocked);
    auth_url_entry.connect_changed(move |entry| {
        copy_button_for_url
            .set_sensitive(!copy_close_blocked_for_url.get() && !entry.text().is_empty());
    });

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
    let finish_button = gtk::Button::with_label("提交回调");
    finish_button.add_css_class("suggested-action");
    actions.append(&finish_button);
    content.append(&actions);

    let close_blocked = copy_close_blocked;
    let panel = AuthPanel {
        account_id: account.id.clone(),
        window: dialog.clone(),
        status_label: status_label.clone(),
        auth_url_entry: auth_url_entry.clone(),
        close_button: close_button.clone(),
        copy_auth_url_button: copy_auth_url_button.clone(),
        finish_button: finish_button.clone(),
        close_blocked: Rc::clone(&close_blocked),
    };
    state.auth_panel.replace(Some(panel));

    let copy_auth_url_entry = auth_url_entry.clone();
    let copy_state = Rc::clone(&state);
    copy_auth_url_button.connect_clicked(move |_| {
        let url = copy_auth_url_entry.text();
        if url.is_empty() {
            show_toast(&copy_state, "认证链接尚未生成");
            return;
        }
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&url);
            show_toast(&copy_state, "认证链接已复制");
        } else {
            show_toast(&copy_state, "无法访问剪贴板");
        }
    });

    let finish_state = Rc::clone(&state);
    let finish_account = account.clone();
    let finish_auth_response_entry = auth_response_entry.clone();
    let finish_status_label = status_label.clone();
    let finish_close_button = close_button.clone();
    let finish_copy_button = copy_auth_url_button.clone();
    let finish_button_for_click = finish_button.clone();
    let finish_close_blocked = Rc::clone(&close_blocked);
    finish_button.connect_clicked(move |_| {
        let response = finish_auth_response_entry.text().trim().to_string();
        if response.is_empty() {
            show_toast(&finish_state, "请粘贴 redirect URI");
            return;
        }
        let response_file = auth_response_path(&finish_account);
        match fs::write(response_file, response) {
            Ok(()) => {
                finish_close_blocked.set(true);
                finish_status_label.set_label("正在认证，请等待 Microsoft 返回结果");
                finish_close_button.set_sensitive(false);
                finish_copy_button.set_sensitive(false);
                finish_button_for_click.set_sensitive(false);
                finish_auth_response_entry.set_editable(false);
                show_toast(&finish_state, "已提交认证回调");
            }
            Err(error) => show_toast(&finish_state, &format!("写入认证回调失败: {error}")),
        }
    });

    let close_state = Rc::clone(&state);
    let close_account_id = account.id.clone();
    let close_blocked_for_button = Rc::clone(&close_blocked);
    close_button.connect_clicked(move |_| {
        if close_blocked_for_button.get() {
            show_toast(&close_state, "正在认证，请等待完成");
            return;
        }
        close_auth_panel(&close_state, &close_account_id);
    });

    let request_close_state = Rc::clone(&state);
    let request_close_account_id = account.id.clone();
    let request_close_blocked = Rc::clone(&close_blocked);
    dialog.connect_close_request(move |_| {
        if request_close_blocked.get() {
            show_toast(&request_close_state, "正在认证，请等待完成");
            return glib::Propagation::Stop;
        }
        close_auth_panel(&request_close_state, &request_close_account_id);
        glib::Propagation::Stop
    });

    dialog.set_content(Some(&root));
    dialog.present();
    start_authentication_flow(
        Rc::clone(&state),
        account,
        auth_url_entry,
        auth_response_entry,
        status_label,
    );
}

fn start_authentication_flow(
    state: Rc<AppState>,
    account: Account,
    auth_url_entry: gtk::Entry,
    auth_response_entry: gtk::Entry,
    status_label: gtk::Label,
) {
    if !Path::new(&account.config_dir).exists()
        && let Err(error) = fs::create_dir_all(&account.config_dir)
    {
        show_toast(&state, &format!("无法创建配置目录: {error}"));
        return;
    }
    if !begin_active_operation(&state, &account.id, ActiveOperation::Authentication) {
        return;
    }

    update_account_status(&state, &account.id, AccountStatus::Authenticating);
    auth_url_entry.set_text("");
    auth_response_entry.set_text("");
    status_label.set_label("正在生成认证链接");
    start_authentication(
        account.clone(),
        onedrive_command(&state),
        state.sender.clone(),
    );
}

fn form_widget_row(label: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
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
    row.append(widget);
    widget.set_hexpand(true);
    row
}
