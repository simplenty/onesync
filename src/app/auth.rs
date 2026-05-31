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
use std::{fs, path::Path, rc::Rc};

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
