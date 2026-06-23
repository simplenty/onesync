use super::super::{
    account_label,
    actions::load_sync_mode_for_selected_profile,
    can_mutate_profile,
    dialogs::auth::show_auth_dialog,
    dialogs::confirm,
    present::backend_error_message,
    render::{rebuild_profile_list, refresh_content, show_toast},
    state::AppState,
    status_label,
    widgets::form_row,
};
use crate::event::BackendError;
use crate::profile::remove_profile_sync_mode;
use crate::profile::{
    Account, AccountStatus, ConfigEdit, OneDriveConfig, read_sync_list,
    remove_confirmation_matches, save_profile_edit, suggested_account_name, suggested_sync_dir,
};
use crate::utils::expand_home;
use adw::prelude::*;
use gtk::{Align, glib};
use std::{cell::Cell, fs, rc::Rc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncDirectionChoice {
    Bidirectional,
    DownloadOnly,
    UploadOnly,
}

impl SyncDirectionChoice {
    fn from_edit(edit: &ConfigEdit) -> Self {
        if edit.download_only {
            Self::DownloadOnly
        } else if edit.upload_only {
            Self::UploadOnly
        } else {
            Self::Bidirectional
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeChoice {
    All,
    Exclude,
    Include,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditPage {
    Overview,
    AccountLocation,
    SyncScope,
    SyncDirection,
    Monitor,
    Remove,
}

impl EditPage {
    fn from_stack_name(name: &str) -> Self {
        match name {
            "account-location" => Self::AccountLocation,
            "sync-scope" => Self::SyncScope,
            "sync-direction" => Self::SyncDirection,
            "monitor" => Self::Monitor,
            "remove" => Self::Remove,
            _ => Self::Overview,
        }
    }
    fn is_sub_page(self) -> bool {
        !matches!(self, Self::Overview)
    }
}

fn direction_title(choice: SyncDirectionChoice, no_remote_delete: bool) -> String {
    match choice {
        SyncDirectionChoice::Bidirectional => "双向同步".to_string(),
        SyncDirectionChoice::DownloadOnly => "只下载到本机".to_string(),
        SyncDirectionChoice::UploadOnly if no_remote_delete => {
            "只上传到 OneDrive · 保护云端文件".to_string()
        }
        SyncDirectionChoice::UploadOnly => "只上传到 OneDrive".to_string(),
    }
}

fn scope_summary(edit: &ConfigEdit) -> String {
    let sync_list_count = split_lines(&edit.sync_list).len();
    if sync_list_count > 0 {
        return format!("只同步 {sync_list_count} 个路径");
    }
    if !edit.skip_file.is_empty() || !edit.skip_dir.is_empty() {
        return format!(
            "忽略 {} 个文件规则，{} 个目录规则",
            edit.skip_file.len(),
            edit.skip_dir.len()
        );
    }
    "全部内容".to_string()
}

fn monitor_summary(edit: &ConfigEdit) -> String {
    let interval = edit.monitor_interval.trim();
    let fullscan = edit.monitor_fullscan_frequency.trim();
    match (interval.is_empty(), fullscan.is_empty()) {
        (true, true) => "使用 onedrive 默认值".to_string(),
        (false, true) => format!("每 {interval} 秒检查"),
        (true, false) => format!("每 {fullscan} 次检查后完整扫描"),
        (false, false) => format!("每 {interval} 秒检查 · 每 {fullscan} 次检查后完整扫描"),
    }
}

fn account_location_summary(account: &Account) -> String {
    format!("{}·{}", account.name, status_label(&account.status))
}

fn refresh_profile_overview_rows(
    account: &Account,
    edit: &ConfigEdit,
    account_location_row: &adw::ActionRow,
    scope_row: &adw::ActionRow,
    direction_row: &adw::ActionRow,
    monitor_row: &adw::ActionRow,
) {
    account_location_row.set_subtitle(account_location_summary(account).as_ref());
    scope_row.set_subtitle(scope_summary(edit).as_ref());
    direction_row.set_subtitle(
        direction_title(SyncDirectionChoice::from_edit(edit), edit.no_remote_delete).as_ref(),
    );
    monitor_row.set_subtitle(monitor_summary(edit).as_ref());
}

fn direction_choice_row(
    title: &str,
    subtitle: &str,
    active: bool,
    can_mutate: bool,
) -> (adw::ActionRow, gtk::CheckButton) {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .activatable(can_mutate)
        .build();
    let check = gtk::CheckButton::builder()
        .active(active)
        .sensitive(can_mutate)
        .valign(Align::Center)
        .build();
    row.add_prefix(&check);
    (row, check)
}

fn monitor_spin_row(
    title: &str,
    subtitle: &str,
    value: &str,
    fallback: f64,
    minimum: f64,
    step: f64,
    can_mutate: bool,
) -> adw::SpinRow {
    let current = value.trim().parse::<f64>().unwrap_or(fallback).max(minimum);
    let adjustment = gtk::Adjustment::new(current, minimum, 86_400.0, step, step * 10.0, 0.0);
    let row = adw::SpinRow::new(Some(&adjustment), 1.0, 0);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_sensitive(can_mutate);
    row
}

fn selected_direction_from_checks(
    _bidirectional_check: &gtk::CheckButton,
    download_check: &gtk::CheckButton,
    upload_check: &gtk::CheckButton,
) -> SyncDirectionChoice {
    if download_check.is_active() {
        SyncDirectionChoice::DownloadOnly
    } else if upload_check.is_active() {
        SyncDirectionChoice::UploadOnly
    } else {
        SyncDirectionChoice::Bidirectional
    }
}

fn selected_scope_from_checks(
    _all_check: &gtk::CheckButton,
    exclude_check: &gtk::CheckButton,
    include_check: &gtk::CheckButton,
) -> ScopeChoice {
    if exclude_check.is_active() {
        ScopeChoice::Exclude
    } else if include_check.is_active() {
        ScopeChoice::Include
    } else {
        ScopeChoice::All
    }
}

pub(in crate::app) fn show_add_account_dialog(state: Rc<AppState>) {
    let dialog = adw::Window::builder()
        .title("添加账号")
        .transient_for(&state.window)
        .modal(true)
        .default_width(480)
        .resizable(false)
        .build();

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let cancel_button = gtk::Button::builder()
        .label("取消")
        .width_request(80)
        .build();
    let add_button = gtk::Button::builder()
        .label("继续")
        .css_classes(["suggested-action"])
        .width_request(80)
        .build();

    let header = adw::HeaderBar::new();
    header.pack_start(&cancel_button);
    header.pack_end(&add_button);
    header.set_show_end_title_buttons(false);
    root.append(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    root.append(&content);

    let name_entry = gtk::Entry::builder()
        .placeholder_text("账户名称，例如 个人")
        .text(suggested_account_name())
        .build();
    let sync_dir_entry = gtk::Entry::builder()
        .placeholder_text("本地同步目录")
        .text(suggested_sync_dir())
        .build();

    content.append(&form_row("名称", &name_entry));
    content.append(&form_row("同步目录", &sync_dir_entry));
    let name_for_check = name_entry.clone();
    let sync_dir_for_check = sync_dir_entry.clone();
    let add_for_check = add_button.clone();
    let check_fields = Rc::new(move || {
        let name_ok = !name_for_check.text().trim().is_empty();
        let dir_ok = !sync_dir_for_check.text().trim().is_empty();
        let valid = name_ok && dir_ok;
        add_for_check.set_sensitive(valid);
        if valid {
            add_for_check.add_css_class("suggested-action");
        } else {
            add_for_check.remove_css_class("suggested-action");
        }
    });
    {
        let check = Rc::clone(&check_fields);
        name_entry.connect_changed(move |_| check());
    }
    {
        let check = Rc::clone(&check_fields);
        sync_dir_entry.connect_changed(move |_| check());
    }
    let cancel_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        cancel_dialog.close();
    });

    let dialog_state = Rc::clone(&state);
    let add_dialog = dialog.clone();
    add_button.connect_clicked(move |_| {
        let name = name_entry.text().trim().to_string();
        let sync_dir = sync_dir_entry.text().trim().to_string();
        let account_result = dialog_state
            .store
            .borrow_mut()
            .add(&name, "", &sync_dir)
            .map_err(BackendError::from);
        match account_result {
            Ok(account) => {
                let auth_account = account.clone();
                let last_index = dialog_state
                    .store
                    .borrow()
                    .accounts()
                    .len()
                    .saturating_sub(1);
                dialog_state.selected_index.set(last_index);
                rebuild_profile_list(&dialog_state);
                load_sync_mode_for_selected_profile(&dialog_state);
                refresh_content(&dialog_state);
                dialog_state.transfers.clear();
                show_toast(&dialog_state, "账号已添加");
                add_dialog.close();
                show_auth_dialog(Rc::clone(&dialog_state), auth_account);
            }
            Err(error) => show_toast(
                &dialog_state,
                &format!("添加账号失败: {}", backend_error_message(&error)),
            ),
        }
    });

    dialog.set_content(Some(&root));
    dialog.present();
}

pub(in crate::app) fn show_edit_profile_dialog(state: Rc<AppState>, account: Account) {
    let dialog = adw::Window::builder()
        .title("编辑账户")
        .transient_for(&state.window)
        .modal(true)
        .default_width(720)
        .default_height(620)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let close_button = gtk::Button::builder()
        .label("关闭")
        .width_request(80)
        .build();
    let save_button = gtk::Button::builder()
        .label("保存")
        .width_request(80)
        .build();
    header.pack_start(&close_button);
    header.pack_end(&save_button);
    root.append(&header);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_vexpand(true);
    root.append(&stack);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(24)
        .margin_start(20)
        .margin_end(20)
        .build();

    let can_mutate = can_mutate_profile(&state, &account);

    save_button.set_sensitive(false);
    let dirty = Rc::new(Cell::new(false));
    let config_path = std::path::Path::new(&account.config_dir).join("config");
    let config_result = OneDriveConfig::read(&config_path);
    let config_available = config_result.is_ok();
    let config_error = config_result.as_ref().err().map(ToString::to_string);
    let mut original_config_edit = config_result
        .as_ref()
        .map(OneDriveConfig::to_edit)
        .unwrap_or_else(|_| ConfigEdit {
            sync_dir: account.sync_dir.clone(),
            ..ConfigEdit::default()
        });
    if let Ok(sync_list) = read_sync_list(&account.config_dir)
        && !sync_list.trim().is_empty()
    {
        original_config_edit.sync_list = sync_list;
    }

    let overview_group = adw::PreferencesGroup::builder().title("账户设置").build();

    let account_location_row = adw::ActionRow::builder()
        .title("账户信息")
        .subtitle(account_location_summary(&account))
        .activatable(can_mutate)
        .build();
    account_location_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    overview_group.add(&account_location_row);

    let scope_row = adw::ActionRow::builder()
        .title("同步范围")
        .subtitle(scope_summary(&original_config_edit))
        .activatable(can_mutate && config_available)
        .build();
    scope_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    overview_group.add(&scope_row);

    let direction_row = adw::ActionRow::builder()
        .title("同步方向")
        .subtitle(direction_title(
            SyncDirectionChoice::from_edit(&original_config_edit),
            original_config_edit.no_remote_delete,
        ))
        .activatable(can_mutate && config_available)
        .build();
    direction_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    overview_group.add(&direction_row);

    let monitor_row = adw::ActionRow::builder()
        .title("自动同步")
        .subtitle(monitor_summary(&original_config_edit))
        .activatable(can_mutate && config_available)
        .build();
    monitor_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    overview_group.add(&monitor_row);

    content.append(&overview_group);

    if let Some(error) = config_error.as_ref() {
        let warning_group = adw::PreferencesGroup::builder()
            .title("Profile 配置不可编辑")
            .description(format!("读取 Profile 配置失败，只能保存账户名称：{error}"))
            .build();
        content.append(&warning_group);
    }

    let edit_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let edit_clamp = adw::Clamp::builder()
        .maximum_size(640)
        .tightening_threshold(560)
        .child(&content)
        .build();
    edit_scrolled.set_child(Some(&edit_clamp));
    stack.add_named(&edit_scrolled, Some("edit"));

    let current_page = Rc::new(Cell::new(EditPage::Overview));
    let remove_mode = Rc::new(Cell::new(false));

    let refresh_header: Rc<dyn Fn()> = Rc::new({
        let save_button = save_button.clone();
        let dirty = Rc::clone(&dirty);
        let current_page = Rc::clone(&current_page);
        let remove_mode = Rc::clone(&remove_mode);
        let close_button = close_button.clone();
        move || {
            let page = current_page.get();
            let in_remove = remove_mode.get();
            close_button.set_label(if page.is_sub_page() {
                "返回"
            } else {
                "关闭"
            });
            if in_remove {
                save_button.set_label("移除账户");
                save_button.remove_css_class("suggested-action");
                save_button.add_css_class("destructive-action");
            } else {
                save_button.set_label("保存");
                save_button.remove_css_class("destructive-action");
                if dirty.get() {
                    save_button.add_css_class("suggested-action");
                    save_button.set_sensitive(true);
                } else {
                    save_button.set_sensitive(false);
                }
            }
        }
    });

    let mark_dirty: Rc<dyn Fn()> = Rc::new({
        let dirty = Rc::clone(&dirty);
        let save_button = save_button.clone();
        move || {
            if !dirty.get() {
                dirty.set(true);
                save_button.set_sensitive(true);
                if dirty.get() {
                    save_button.add_css_class("suggested-action");
                    save_button.set_sensitive(true);
                } else {
                    save_button.set_sensitive(false);
                }
            }
        }
    });

    let account_location_page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(24)
        .margin_start(20)
        .margin_end(20)
        .build();
    account_location_page.append(&page_intro(
        "账户信息",
        "修改侧边栏显示名称或同步目录。保存配置不会移动已有文件。",
    ));

    let account_location_group = adw::PreferencesGroup::builder().title("账户信息").build();
    let name_row = adw::EntryRow::builder()
        .title("账户名称")
        .text(&account.name)
        .editable(can_mutate)
        .build();
    let email_row = adw::ActionRow::builder()
        .title("账号标识")
        .subtitle(account_label(&account))
        .build();
    let status_row = adw::ActionRow::builder()
        .title("认证状态")
        .subtitle(status_label(&account.status))
        .build();
    let sync_dir_row = adw::EntryRow::builder()
        .title("同步目录")
        .text(&original_config_edit.sync_dir)
        .editable(false)
        .build();
    let sync_dir_button = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text("选择同步目录")
        .sensitive(can_mutate && config_available)
        .valign(Align::Center)
        .build();
    sync_dir_button.add_css_class("flat");
    sync_dir_row.add_suffix(&sync_dir_button);
    account_location_group.add(&name_row);
    account_location_group.add(&email_row);
    account_location_group.add(&status_row);
    account_location_group.add(&sync_dir_row);
    account_location_page.append(&account_location_group);

    let account_location_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let account_location_clamp = adw::Clamp::builder()
        .maximum_size(640)
        .tightening_threshold(560)
        .child(&account_location_page)
        .build();
    account_location_scrolled.set_child(Some(&account_location_clamp));
    stack.add_named(&account_location_scrolled, Some("account-location"));

    let cp_for_account_location = Rc::clone(&current_page);
    let refresh_for_account_location = Rc::clone(&refresh_header);
    let stack_for_account_location = stack.clone();
    account_location_row.connect_activated(move |_| {
        stack_for_account_location.set_visible_child_name("account-location");
        cp_for_account_location.set(EditPage::AccountLocation);
        (refresh_for_account_location)();
    });

    let sync_dir_dialog = dialog.clone();
    let sync_dir_row_for_button = sync_dir_row.clone();
    sync_dir_button.connect_clicked(move |_| {
        let warning = adw::AlertDialog::new(
            Some("更改同步目录?"),
            Some(
                "OneSync 只会修改当前 Profile 的 sync_dir 配置，不会移动已有文件。请选择已经准备好的目录；如果目录内容与 OneDrive 状态不一致，后续同步可能需要重新确认或重新同步状态。",
            ),
        );
        warning.add_responses(&[("cancel", "取消"), ("choose", "继续选择")]);
        warning.set_default_response(Some("cancel"));
        warning.set_close_response("cancel");
        let dialog_for_file = sync_dir_dialog.clone();
        let row_for_file = sync_dir_row_for_button.clone();
        warning.choose(
            Some(&sync_dir_dialog),
            None::<&gtk::gio::Cancellable>,
            move |response| {
                if response != "choose" {
                    return;
                }
                let file_dialog = gtk::FileDialog::builder().title("选择同步目录").build();
                let row_for_result = row_for_file.clone();
                file_dialog.select_folder(
                    Some(&dialog_for_file),
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        if let Ok(folder) = result
                            && let Some(path) = folder.path()
                        {
                            row_for_result.set_text(&path.to_string_lossy());
                        }
                    },
                );
            },
        );
    });

    let scope_page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(20)
        .margin_bottom(24)
        .margin_start(20)
        .margin_end(20)
        .build();
    scope_page.append(&page_intro(
        "同步范围",
        "决定这个 Profile 参与同步的路径集合。范围变化后通常需要重新同步状态。",
    ));

    let initial_scope = if !original_config_edit.sync_list.trim().is_empty() {
        ScopeChoice::Include
    } else if !original_config_edit.skip_file.is_empty()
        || !original_config_edit.skip_dir.is_empty()
    {
        ScopeChoice::Exclude
    } else {
        ScopeChoice::All
    };
    let scope_group = adw::PreferencesGroup::builder().title("同步范围").build();
    let (all_choice_row, all_check) = direction_choice_row(
        "全部同步",
        "同步目录中的所有内容；onedrive 仍会应用自身默认的临时文件忽略规则。",
        matches!(initial_scope, ScopeChoice::All),
        can_mutate && config_available,
    );
    let (exclude_choice_row, exclude_check) = direction_choice_row(
        "忽略内容",
        "用 skip_file 和 skip_dir 排除不需要同步的文件或目录。",
        matches!(initial_scope, ScopeChoice::Exclude),
        can_mutate && config_available,
    );
    let (include_choice_row, include_check) = direction_choice_row(
        "只同步指定",
        "仅同步 sync_list 中列出的相对路径。",
        matches!(initial_scope, ScopeChoice::Include),
        can_mutate && config_available,
    );
    exclude_check.set_group(Some(&all_check));
    include_check.set_group(Some(&all_check));
    scope_group.add(&all_choice_row);
    scope_group.add(&exclude_choice_row);
    scope_group.add(&include_choice_row);
    scope_page.append(&scope_group);

    let all_content = scope_section_box(
        "全部同步",
        Some("同步目录中的所有内容；onedrive 仍会应用自身默认的临时文件忽略规则。"),
    );

    let exclude_scope_page = scope_section_box(
        "忽略内容",
        Some("用 skip_file 和 skip_dir 排除不需要同步的文件或目录。"),
    );
    let (exclude_file_page, skip_file_list, _skip_file_entry) = build_rule_editor(
        "忽略文件",
        "每条规则匹配不需要同步的文件，例如 *.tmp 或 *.swp。自定义规则会覆盖 onedrive 默认文件忽略规则。",
        "例如 *.tmp",
        &original_config_edit.skip_file,
    );
    let (exclude_dir_page, skip_dir_list, _skip_dir_entry) = build_rule_editor(
        "忽略目录",
        "每条规则是相对于同步目录的目录名或路径，例如 node_modules、.git 或 target。",
        "例如 node_modules",
        &original_config_edit.skip_dir,
    );
    exclude_scope_page.append(&exclude_file_page);
    exclude_scope_page.append(&exclude_dir_page);

    let include_values = split_lines(&original_config_edit.sync_list);
    let include_scope_page =
        scope_section_box("只同步指定", Some("仅同步 sync_list 中列出的相对路径。"));
    let (include_paths_page, sync_list_list, _sync_list_entry) = build_rule_editor(
        "同步路径",
        "每行一个相对于同步目录的路径，例如 Documents 或 Pictures/Trips。留空表示不启用选择性同步。",
        "例如 Documents",
        &include_values,
    );
    include_scope_page.append(&include_paths_page);

    let initial_is_exclude = matches!(initial_scope, ScopeChoice::Exclude);
    let initial_is_include = matches!(initial_scope, ScopeChoice::Include);
    all_content.set_visible(!initial_is_exclude && !initial_is_include);
    exclude_scope_page.set_visible(initial_is_exclude);
    include_scope_page.set_visible(initial_is_include);
    scope_page.append(&all_content);
    scope_page.append(&exclude_scope_page);
    scope_page.append(&include_scope_page);

    let all_content_show = all_content.clone();
    let excl_content_show = exclude_scope_page.clone();
    let incl_content_show = include_scope_page.clone();
    all_check.connect_toggled(move |check| {
        if check.is_active() {
            all_content_show.set_visible(true);
            excl_content_show.set_visible(false);
            incl_content_show.set_visible(false);
        }
    });

    let all_content_hide = all_content.clone();
    let excl_content_show2 = exclude_scope_page.clone();
    let incl_content_hide = include_scope_page.clone();
    exclude_check.connect_toggled(move |check| {
        if check.is_active() {
            all_content_hide.set_visible(false);
            excl_content_show2.set_visible(true);
            incl_content_hide.set_visible(false);
        }
    });

    let all_content_hide2 = all_content.clone();
    let excl_content_hide = exclude_scope_page.clone();
    let incl_content_show2 = include_scope_page.clone();
    include_check.connect_toggled(move |check| {
        if check.is_active() {
            all_content_hide2.set_visible(false);
            excl_content_hide.set_visible(false);
            incl_content_show2.set_visible(true);
        }
    });

    let all_check_row = all_check.clone();
    all_choice_row.connect_activated(move |_| {
        all_check_row.set_active(true);
    });
    let exclude_check_row = exclude_check.clone();
    exclude_choice_row.connect_activated(move |_| {
        exclude_check_row.set_active(true);
    });
    let include_check_row = include_check.clone();
    include_choice_row.connect_activated(move |_| {
        include_check_row.set_active(true);
    });

    let scope_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let scope_clamp = adw::Clamp::builder()
        .maximum_size(640)
        .tightening_threshold(560)
        .child(&scope_page)
        .build();
    scope_scrolled.set_child(Some(&scope_clamp));
    stack.add_named(&scope_scrolled, Some("sync-scope"));

    let cp_for_sync_scope = Rc::clone(&current_page);
    let refresh_for_sync_scope = Rc::clone(&refresh_header);
    let stack_for_scope = stack.clone();
    scope_row.connect_activated(move |_| {
        stack_for_scope.set_visible_child_name("sync-scope");
        cp_for_sync_scope.set(EditPage::SyncScope);
        (refresh_for_sync_scope)();
    });
    let direction_page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(24)
        .margin_start(20)
        .margin_end(20)
        .build();
    direction_page.append(&page_intro(
        "同步方向",
        "限制本地目录和 OneDrive 之间允许传播的变更类型。",
    ));

    let initial_direction = SyncDirectionChoice::from_edit(&original_config_edit);
    let direction_group = adw::PreferencesGroup::builder().title("同步方向").build();
    let (bidirectional_row, bidirectional_check) = direction_choice_row(
        "双向同步",
        "本地和 OneDrive 的新增、修改、删除都会互相同步。",
        matches!(initial_direction, SyncDirectionChoice::Bidirectional),
        can_mutate && config_available,
    );
    let (download_row, download_check) = direction_choice_row(
        "只下载到本机",
        "从 OneDrive 下载更改，不上传本地更改。",
        matches!(initial_direction, SyncDirectionChoice::DownloadOnly),
        can_mutate && config_available,
    );
    let (upload_row, upload_check) = direction_choice_row(
        "只上传到 OneDrive",
        "上传本地更改，不下载 OneDrive 更改。",
        matches!(initial_direction, SyncDirectionChoice::UploadOnly),
        can_mutate && config_available,
    );
    download_check.set_group(Some(&bidirectional_check));
    upload_check.set_group(Some(&bidirectional_check));
    direction_group.add(&bidirectional_row);
    direction_group.add(&download_row);
    direction_group.add(&upload_row);
    direction_page.append(&direction_group);

    let no_remote_delete_row = adw::ActionRow::builder()
        .title("保护云端文件")
        .subtitle("启用后，本地删除不会删除 OneDrive 文件。")
        .visible(matches!(initial_direction, SyncDirectionChoice::UploadOnly))
        .activatable(can_mutate && config_available)
        .build();
    let no_remote_delete_switch = gtk::Switch::builder()
        .active(original_config_edit.no_remote_delete)
        .sensitive(can_mutate && config_available)
        .valign(Align::Center)
        .build();
    no_remote_delete_row.add_suffix(&no_remote_delete_switch);
    no_remote_delete_row.set_activatable_widget(Some(&no_remote_delete_switch));
    direction_group.add(&no_remote_delete_row);
    let no_remote_delete_for_upload = no_remote_delete_row.clone();
    upload_check.connect_toggled(move |check| {
        no_remote_delete_for_upload.set_visible(check.is_active());
    });
    let bidirectional_check_for_row = bidirectional_check.clone();
    bidirectional_row.connect_activated(move |_| {
        bidirectional_check_for_row.set_active(true);
    });
    let download_check_for_row = download_check.clone();
    download_row.connect_activated(move |_| {
        download_check_for_row.set_active(true);
    });
    let upload_check_for_row = upload_check.clone();
    upload_row.connect_activated(move |_| {
        upload_check_for_row.set_active(true);
    });

    let direction_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let direction_clamp = adw::Clamp::builder()
        .maximum_size(640)
        .tightening_threshold(560)
        .child(&direction_page)
        .build();
    direction_scrolled.set_child(Some(&direction_clamp));
    stack.add_named(&direction_scrolled, Some("sync-direction"));

    let cp_for_sync_direction = Rc::clone(&current_page);
    let refresh_for_sync_direction = Rc::clone(&refresh_header);
    let stack_for_direction = stack.clone();
    direction_row.connect_activated(move |_| {
        stack_for_direction.set_visible_child_name("sync-direction");
        cp_for_sync_direction.set(EditPage::SyncDirection);
        (refresh_for_sync_direction)();
    });

    let monitor_page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(24)
        .margin_start(20)
        .margin_end(20)
        .build();
    monitor_page.append(&page_intro(
        "自动同步",
        "这些值只影响持续同步模式。留空表示使用 onedrive 默认值。",
    ));

    let monitor_group = adw::PreferencesGroup::builder().title("自动同步").build();
    let monitor_uses_default = Rc::new(Cell::new(
        original_config_edit.monitor_interval.trim().is_empty()
            && original_config_edit
                .monitor_fullscan_frequency
                .trim()
                .is_empty(),
    ));
    let monitor_interval_row = monitor_spin_row(
        "检查间隔",
        "持续同步每隔多久检查一次变更（单位：秒）。",
        &original_config_edit.monitor_interval,
        300.0,
        1.0,
        30.0,
        can_mutate && config_available,
    );
    let monitor_fullscan_row = monitor_spin_row(
        "完整扫描",
        "每多少次检查后执行一次完整扫描（单位：次）。",
        &original_config_edit.monitor_fullscan_frequency,
        12.0,
        0.0,
        1.0,
        can_mutate && config_available,
    );
    monitor_group.add(&monitor_interval_row);
    monitor_group.add(&monitor_fullscan_row);
    monitor_page.append(&monitor_group);

    let monitor_reset_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(Align::Fill)
        .margin_top(2)
        .build();
    let monitor_default_label = gtk::Label::builder()
        .label(if monitor_uses_default.get() {
            "当前使用 onedrive 默认检查设置"
        } else {
            "当前使用自定义检查设置"
        })
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["dim-label"])
        .build();
    let monitor_reset_button = gtk::Button::with_label("恢复默认");
    monitor_reset_button.set_sensitive(can_mutate && config_available);
    monitor_reset_row.append(&monitor_default_label);
    monitor_reset_row.append(&monitor_reset_button);
    monitor_page.append(&monitor_reset_row);

    let monitor_default_for_interval = Rc::clone(&monitor_uses_default);
    let monitor_label_for_interval = monitor_default_label.clone();
    monitor_interval_row
        .adjustment()
        .connect_value_changed(move |_| {
            monitor_default_for_interval.set(false);
            monitor_label_for_interval.set_label("当前使用自定义检查设置");
        });

    let monitor_default_for_fullscan = Rc::clone(&monitor_uses_default);
    let monitor_label_for_fullscan = monitor_default_label.clone();
    monitor_fullscan_row
        .adjustment()
        .connect_value_changed(move |_| {
            monitor_default_for_fullscan.set(false);
            monitor_label_for_fullscan.set_label("当前使用自定义检查设置");
        });

    let monitor_default_for_reset = Rc::clone(&monitor_uses_default);
    let monitor_interval_for_reset = monitor_interval_row.clone();
    let monitor_fullscan_for_reset = monitor_fullscan_row.clone();
    let monitor_label_for_reset = monitor_default_label.clone();
    monitor_reset_button.connect_clicked(move |_| {
        monitor_interval_for_reset.set_value(300.0);
        monitor_fullscan_for_reset.set_value(12.0);
        monitor_default_for_reset.set(true);
        monitor_label_for_reset.set_label("当前使用 onedrive 默认检查设置");
    });

    let monitor_scrolled = gtk::ScrolledWindow::builder().vexpand(true).build();
    let monitor_clamp = adw::Clamp::builder()
        .maximum_size(640)
        .tightening_threshold(560)
        .child(&monitor_page)
        .build();
    monitor_scrolled.set_child(Some(&monitor_clamp));
    stack.add_named(&monitor_scrolled, Some("monitor"));
    {
        let md = Rc::clone(&mark_dirty);
        name_row.connect_changed(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        all_check.connect_toggled(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        exclude_check.connect_toggled(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        include_check.connect_toggled(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        bidirectional_check.connect_toggled(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        download_check.connect_toggled(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        upload_check.connect_toggled(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        no_remote_delete_switch.connect_activate(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        monitor_interval_row
            .adjustment()
            .connect_value_changed(move |_| md());
    }
    {
        let md = Rc::clone(&mark_dirty);
        monitor_fullscan_row
            .adjustment()
            .connect_value_changed(move |_| md());
    }

    let cp_for_monitor = Rc::clone(&current_page);
    let refresh_for_monitor = Rc::clone(&refresh_header);
    let stack_for_monitor = stack.clone();
    monitor_row.connect_activated(move |_| {
        stack_for_monitor.set_visible_child_name("monitor");
        cp_for_monitor.set(EditPage::Monitor);
        (refresh_for_monitor)();
    });

    let danger_group = adw::PreferencesGroup::builder().title("危险操作").build();
    let remove_row = adw::ActionRow::builder()
        .title("移除 Profile")
        .subtitle("只从 OneSync 移除，不删除云端或本地文件")
        .activatable(can_mutate || !matches!(account.status, AccountStatus::Authenticated))
        .build();
    remove_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    danger_group.add(&remove_row);
    danger_group.set_visible(can_mutate || !matches!(account.status, AccountStatus::Authenticated));
    content.append(&danger_group);

    let remove_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .hexpand(true)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let remove_title = gtk::Label::builder()
        .label(format!("移除账户 \"{}\"?", account.name))
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["title-3"])
        .build();
    remove_content.append(&remove_title);

    let risk_label = gtk::Label::builder()
        .label("移除后，这个账号不会再出现在列表里，也不会继续从这个账户启动同步。")
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .build();
    remove_content.append(&risk_label);

    let guarantee_label = gtk::Label::builder()
        .label("这不会删除 OneDrive 云端文件。")
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["dim-label"])
        .build();
    remove_content.append(&guarantee_label);

    let instruction = gtk::Label::builder()
        .label(format!("请输入 \"{}\" 以确认移除。", account.name))
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let remove_name_entry = gtk::Entry::builder()
        .placeholder_text(&account.name)
        .width_chars(32)
        .max_width_chars(48)
        .halign(Align::Fill)
        .hexpand(true)
        .build();
    let confirm_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .build();
    confirm_box.append(&instruction);
    confirm_box.append(&remove_name_entry);
    remove_content.append(&confirm_box);

    stack.add_named(&remove_content, Some("remove"));
    stack.set_visible_child_name("edit");

    let expected_remove_name = account.name.clone();
    let _close_save_button = save_button.clone();
    let close_stack = stack.clone();
    let close_dialog = dialog.clone();
    let close_current = Rc::clone(&current_page);
    let close_remove = Rc::clone(&remove_mode);
    let close_refresh = Rc::clone(&refresh_header);
    close_button.connect_clicked(move |_| {
        let name = close_stack
            .visible_child_name()
            .as_deref()
            .unwrap_or("")
            .to_string();
        let page = EditPage::from_stack_name(&name);
        if close_remove.get() {
            close_remove.set(false);
            close_stack.set_visible_child_name("edit");
            close_current.set(EditPage::Overview);
            (close_refresh)();
            return;
        }
        if page.is_sub_page() {
            close_stack.set_visible_child_name("edit");
            close_current.set(EditPage::Overview);
            (close_refresh)();
            return;
        }
        close_dialog.close();
    });
    let save_state = Rc::clone(&state);
    let save_dialog = dialog.clone();
    let save_account_id = account.id.clone();
    let save_mode = Rc::clone(&remove_mode);
    let save_account = account.clone();
    let save_expected_remove_name = expected_remove_name.clone();
    let save_remove_name_entry = remove_name_entry.clone();
    save_button.connect_clicked(move |_| {
        if save_mode.get() {
            if !remove_confirmation_matches(
                &save_expected_remove_name,
                &save_remove_name_entry.text(),
            ) {
                show_toast(&save_state, "请输入完整账户名称");
                return;
            }
            remove_profile(&save_state, &save_account);
            save_dialog.close();
            return;
        }

        let next = name_row.text().trim().to_string();
        if next.is_empty() {
            show_toast(&save_state, "名称不能为空");
            return;
        }
        if save_state
            .store
            .borrow()
            .accounts()
            .iter()
            .any(|stored| stored.id != save_account_id && stored.name == next)
        {
            show_toast(&save_state, "账户名称已存在");
            return;
        }
        let next_config_edit = if config_available {
            match collect_config_edit(
                &original_config_edit,
                &sync_dir_row,
                &all_check,
                &exclude_check,
                &include_check,
                &skip_file_list,
                &skip_dir_list,
                &sync_list_list,
                &bidirectional_check,
                &download_check,
                &upload_check,
                &no_remote_delete_switch,
                &monitor_interval_row,
                &monitor_fullscan_row,
                &monitor_uses_default,
            ) {
                Ok(edit) => Some(edit),
                Err(message) => {
                    show_toast(&save_state, &message);
                    return;
                }
            }
        } else {
            None
        };
        if let Some(stored) = save_state
            .store
            .borrow_mut()
            .accounts_mut()
            .iter_mut()
            .find(|stored| stored.id == save_account_id)
        {
            stored.name = next.clone();
            if let Some(next_config_edit) = next_config_edit.as_ref() {
                stored.sync_dir = next_config_edit.sync_dir.clone();
            }
        }
        let mut display_account = save_account.clone();
        display_account.name = next.clone();
        if let Some(next_config_edit) = next_config_edit.as_ref() {
            display_account.sync_dir = next_config_edit.sync_dir.clone();
            refresh_profile_overview_rows(
                &display_account,
                next_config_edit,
                &account_location_row,
                &scope_row,
                &direction_row,
                &monitor_row,
            );
        }
        if let Err(error) = save_state.store.borrow().flush() {
            show_toast(&save_state, &format!("保存账号失败: {error}"));
            return;
        }
        let mut needs_resync = false;
        let mut sync_dir_changed = false;
        if let Some(next_config_edit) = next_config_edit.as_ref() {
            match save_profile_edit(&save_account, &original_config_edit, next_config_edit) {
                Ok(outcome) => {
                    needs_resync = outcome.needs_resync;
                    sync_dir_changed = outcome.sync_dir_changed;
                }
                Err(error) => {
                    show_toast(&save_state, &error.to_string());
                    return;
                }
            }
        }
        rebuild_profile_list(&save_state);
        refresh_content(&save_state);
        save_dialog.close();
        if sync_dir_changed || needs_resync {
            let resync_state = save_state.clone();
            let resync_account = save_account.clone();
            glib::idle_add_local(move || {
                confirm::show_resync_confirmation(resync_state.clone(), resync_account.clone());
                glib::ControlFlow::Break
            });
        } else {
            show_toast(&save_state, "Profile 已保存");
        }
    });

    let remove_state = Rc::clone(&state);
    let remove_account = account.clone();
    let remove_stack = stack.clone();
    let _remove_close_button = close_button.clone();
    let remove_save_button = save_button.clone();
    let remove_mode_for_row = Rc::clone(&remove_mode);
    let remove_entry_for_row = remove_name_entry.clone();
    let expected_remove_name_for_row = expected_remove_name.clone();
    let remove_refresh = Rc::clone(&refresh_header);
    let current_page_remove = Rc::clone(&current_page);
    remove_row.connect_activated(move |_| {
        if !can_mutate_profile(&remove_state, &remove_account) {
            show_toast(&remove_state, "请先停止该账户的认证、同步或持续同步");
            return;
        }
        remove_mode_for_row.set(true);
        remove_entry_for_row.set_text("");
        current_page_remove.set(EditPage::Remove);
        remove_stack.set_visible_child_name("remove");
        (remove_refresh)();
        remove_save_button.set_sensitive(remove_confirmation_matches(
            &expected_remove_name_for_row,
            &remove_entry_for_row.text(),
        ));
    });

    let confirm_for_entry = save_button.clone();
    let entry_mode = Rc::clone(&remove_mode);
    let expected_name_for_entry = expected_remove_name.clone();
    remove_name_entry.connect_changed(move |entry| {
        if entry_mode.get() {
            confirm_for_entry.set_sensitive(remove_confirmation_matches(
                &expected_name_for_entry,
                &entry.text(),
            ));
        }
    });

    dialog.set_content(Some(&root));
    dialog.present();
}

fn scope_section_box(title: &str, subtitle: Option<&str>) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .build();
    header.append(
        &gtk::Label::builder()
            .label(title)
            .halign(Align::Start)
            .css_classes(["heading"])
            .build(),
    );
    if let Some(subtitle) = subtitle {
        header.append(&help_label(subtitle));
    }
    root.append(&header);
    root
}

fn page_intro(title: &str, subtitle: &str) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_start(2)
        .margin_end(2)
        .build();
    root.append(
        &gtk::Label::builder()
            .label(title)
            .halign(Align::Start)
            .css_classes(["title-3"])
            .build(),
    );
    root.append(&help_label(subtitle));
    root
}

fn help_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build()
}

fn append_rule_row(list: &gtk::ListBox, value: &str) {
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(8)
        .margin_end(8)
        .build();
    let label = gtk::Label::builder()
        .label(value)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let remove_button = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("移除规则")
        .build();
    remove_button.add_css_class("flat");
    row_box.append(&label);
    row_box.append(&remove_button);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    list.append(&row);

    let row_for_remove = row.clone();
    remove_button.connect_clicked(move |_| {
        if let Some(parent) = row_for_remove
            .parent()
            .and_then(|widget| widget.downcast::<gtk::ListBox>().ok())
        {
            parent.remove(&row_for_remove);
        }
    });
}

fn build_rule_editor(
    title: &str,
    help: &str,
    placeholder: &str,
    values: &[String],
) -> (gtk::Box, gtk::ListBox, gtk::Entry) {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    root.append(
        &gtk::Label::builder()
            .label(title)
            .halign(Align::Start)
            .css_classes(["heading"])
            .build(),
    );
    root.append(&help_label(help));

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_top(2)
        .build();
    list.set_placeholder(Some(
        &gtk::Label::builder()
            .label("暂无规则")
            .margin_top(10)
            .margin_bottom(10)
            .css_classes(["dim-label"])
            .build(),
    ));
    for value in values {
        append_rule_row(&list, value);
    }
    root.append(&list);

    let add_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .hexpand(true)
        .build();
    let add_button = gtk::Button::with_label("添加");
    add_button.add_css_class("flat");
    add_box.append(&entry);
    add_box.append(&add_button);
    root.append(&add_box);

    let list_for_button = list.clone();
    let entry_for_button = entry.clone();
    add_button.connect_clicked(move |_| {
        add_rule_from_entry(&list_for_button, &entry_for_button);
    });

    let list_for_entry = list.clone();
    entry.connect_activate(move |entry| {
        add_rule_from_entry(&list_for_entry, entry);
    });

    (root, list, entry)
}

fn add_rule_from_entry(list: &gtk::ListBox, entry: &gtk::Entry) {
    let value = entry.text().trim().to_string();
    if value.is_empty() || collect_rule_values(list).contains(&value) {
        entry.set_text("");
        return;
    }
    append_rule_row(list, &value);
    entry.set_text("");
}

fn collect_rule_values(list: &gtk::ListBox) -> Vec<String> {
    let mut values = Vec::new();
    let mut child = list.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if let Ok(row) = widget.downcast::<gtk::ListBoxRow>()
            && let Some(row_box) = row
                .child()
                .and_then(|widget| widget.downcast::<gtk::Box>().ok())
            && let Some(label) = row_box
                .first_child()
                .and_then(|widget| widget.downcast::<gtk::Label>().ok())
        {
            let value = label.text().trim().to_string();
            if !value.is_empty() && !values.contains(&value) {
                values.push(value);
            }
        }
        child = next;
    }
    values
}

fn split_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

// ponytail: 15 widget refs are UI-glue; grouping into a struct is a separate UI refactor
#[allow(clippy::too_many_arguments)]
fn collect_config_edit(
    original: &ConfigEdit,
    sync_dir_row: &adw::EntryRow,
    all_check: &gtk::CheckButton,
    exclude_check: &gtk::CheckButton,
    include_check: &gtk::CheckButton,
    skip_file_list: &gtk::ListBox,
    skip_dir_list: &gtk::ListBox,
    sync_list_list: &gtk::ListBox,
    bidirectional_check: &gtk::CheckButton,
    download_check: &gtk::CheckButton,
    upload_check: &gtk::CheckButton,
    no_remote_delete_switch: &gtk::Switch,
    monitor_interval_row: &adw::SpinRow,
    monitor_fullscan_row: &adw::SpinRow,
    monitor_uses_default: &Cell<bool>,
) -> Result<ConfigEdit, String> {
    let sync_dir = sync_dir_row.text().trim().to_string();
    if sync_dir.is_empty() {
        return Err("同步目录不能为空".to_string());
    }

    let monitor_interval = if monitor_uses_default.get() {
        String::new()
    } else {
        (monitor_interval_row.value().round() as i32)
            .max(1)
            .to_string()
    };
    let monitor_fullscan_frequency = if monitor_uses_default.get() {
        String::new()
    } else {
        (monitor_fullscan_row.value().round() as i32)
            .max(0)
            .to_string()
    };

    let mut edit = original.clone();
    edit.sync_dir = sync_dir;
    edit.monitor_interval = monitor_interval;
    edit.monitor_fullscan_frequency = monitor_fullscan_frequency;

    match selected_scope_from_checks(all_check, exclude_check, include_check) {
        ScopeChoice::Exclude => {
            edit.skip_file = collect_rule_values(skip_file_list);
            edit.skip_dir = collect_rule_values(skip_dir_list);
            edit.sync_list.clear();
        }
        ScopeChoice::Include => {
            edit.skip_file.clear();
            edit.skip_dir.clear();
            edit.sync_list = collect_rule_values(sync_list_list).join("\n");
        }
        ScopeChoice::All => {
            edit.skip_file.clear();
            edit.skip_dir.clear();
            edit.sync_list.clear();
        }
    }

    match selected_direction_from_checks(bidirectional_check, download_check, upload_check) {
        SyncDirectionChoice::Bidirectional => {
            edit.download_only = false;
            edit.upload_only = false;
            edit.no_remote_delete = false;
        }
        SyncDirectionChoice::DownloadOnly => {
            edit.download_only = true;
            edit.upload_only = false;
            edit.no_remote_delete = false;
        }
        SyncDirectionChoice::UploadOnly => {
            edit.download_only = false;
            edit.upload_only = true;
            edit.no_remote_delete = no_remote_delete_switch.is_active();
        }
    }

    Ok(edit)
}

fn remove_profile(state: &Rc<AppState>, account: &Account) {
    let selected = state.selected_index.get();
    if let Err(error) = state.store.borrow_mut().remove(&account.id) {
        show_toast(state, &format!("保存账号失败: {error}"));
    }
    state.selected_index.set(selected.saturating_sub(1));
    if let Err(error) = remove_profile_sync_mode(&account.id) {
        show_toast(state, &format!("删除同步模式设置失败: {error}"));
    }
    rebuild_profile_list(state);
    load_sync_mode_for_selected_profile(state);
    refresh_content(state);
    state.transfers.clear();

    let dialog = adw::AlertDialog::new(
        Some("移除本地同步目录?"),
        Some(&format!(
            "已从 OneSync 移除账户 {}。\n是否同时删除本地同步目录？",
            account.name
        )),
    );
    dialog.add_responses(&[("keep", "只移除账户"), ("delete", "同时删除目录")]);
    dialog.set_default_response(Some("keep"));
    dialog.set_close_response("keep");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    let dialog_state = Rc::clone(state);
    let dialog_account = account.clone();
    dialog.choose(
        Some(&state.window),
        None::<&gtk::gio::Cancellable>,
        move |response| {
            if response != "delete" {
                return;
            }
            let sync_path = expand_home(&dialog_account.sync_dir);
            if sync_path.exists() {
                if let Err(error) = fs::remove_dir_all(&sync_path) {
                    show_toast(&dialog_state, &format!("删除同步目录失败: {error}"));
                } else {
                    show_toast(&dialog_state, "本地同步目录已删除");
                }
            }
        },
    );
}
