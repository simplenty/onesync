use super::{
    auth::show_auth_dialog,
    begin_active_operation, can_mutate_profile,
    confirm::show_confirmation,
    ensure_client_ready, onedrive_command,
    render::{rebuild_profile_list, refresh_content, show_toast},
    state::{ActiveOperation, AppState},
    status::{account_label, status_label},
    widgets::form_row,
};
use crate::{
    account::{
        Account, create_account, remove_confirmation_matches, save_accounts,
        suggested_account_name, suggested_sync_dir,
    },
    onedrive::start_logout,
};
use adw::prelude::*;
use gtk::Align;
use std::rc::Rc;

pub(in crate::app) fn show_add_account_dialog(state: Rc<AppState>) {
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
        .text(suggested_account_name())
        .build();
    let email_entry = gtk::Entry::builder()
        .placeholder_text("账号标识，例如 john@example.com")
        .build();
    let sync_dir_entry = gtk::Entry::builder()
        .placeholder_text("本地同步目录")
        .text(suggested_sync_dir())
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

pub(in crate::app) fn show_edit_profile_dialog(state: Rc<AppState>, account: Account) {
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
