use crate::app::{
    change_kind_icon, file_display_state, preview_change_description, preview_intent_detail,
    preview_intent_label,
};
use crate::event::payload::{FileChange, PreviewChange, PreviewState};
use gtk::{Align, prelude::*};
use std::{
    cell::{Cell, RefCell},
    cmp::Reverse,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

const FILE_COLUMN_WIDTH: i32 = 220;
const PROGRESS_ANIMATION_MS: u64 = 240;

#[derive(Clone)]
struct TransferRow {
    list_row: gtk::ListBoxRow,
    state_label: gtk::Label,
    progress_bar: gtk::ProgressBar,
    progress_animation: Rc<Cell<u64>>,
    started_order: Cell<u64>,
    target_progress: Cell<f64>,
    completed: Cell<bool>,
}

#[derive(Clone)]
enum ListRow {
    Live(TransferRow),
    Preview(PreviewRow),
}

#[derive(Clone)]
struct PreviewRow {
    list_row: gtk::ListBoxRow,
    state_label: gtk::Label,
    progress_bar: gtk::ProgressBar,
    progress_animation: Rc<Cell<u64>>,
    accept_button: gtk::Button,
    dismiss_button: gtk::Button,
    started_order: Cell<u64>,
    state: Cell<PreviewState>,
}

type PreviewCallback = Rc<dyn Fn(String, String)>;

pub(in crate::app) struct TransferList {
    list: gtk::ListBox,
    rows: RefCell<HashMap<String, ListRow>>,
    next_order: Cell<u64>,
    accept_preview: RefCell<Option<PreviewCallback>>,
    dismiss_preview: RefCell<Option<PreviewCallback>>,
}

impl TransferList {
    pub(in crate::app) fn new(list: gtk::ListBox) -> Self {
        Self {
            list,
            rows: RefCell::new(HashMap::new()),
            next_order: Cell::new(0),
            accept_preview: RefCell::new(None),
            dismiss_preview: RefCell::new(None),
        }
    }

    pub(in crate::app) fn clear(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.rows.borrow_mut().clear();
        self.next_order.set(0);
    }

    pub(in crate::app) fn upsert(&self, file: FileChange) {
        let completed = file.is_complete();
        if let Some(ListRow::Live(row)) = self.rows.borrow().get(&file.name).cloned() {
            let progress = if file.failed {
                file.progress
            } else if completed {
                1.0
            } else {
                file.progress.max(row.progress_bar.fraction())
            };
            row.state_label.set_label(&file_display_state(&file));
            row.target_progress.set(progress);
            row.completed.set(completed);
            animate_progress(&row, progress);
            self.reorder();
            return;
        }

        let key = file.name.clone();
        let target_progress = if completed { 1.0 } else { file.progress };
        let mut initial_file = file;
        initial_file.progress = 0.0;
        let (row, transfer_row) = build_file_row(initial_file);
        let started_order = self.next_order.get();
        self.next_order.set(started_order.wrapping_add(1));
        transfer_row.started_order.set(started_order);
        transfer_row.target_progress.set(target_progress);
        transfer_row.completed.set(completed);
        self.list.prepend(&row);
        animate_progress(&transfer_row, target_progress);
        self.rows
            .borrow_mut()
            .insert(key, ListRow::Live(transfer_row));
        self.reorder();
    }

    pub(in crate::app) fn upsert_preview(&self, account_id: String, change: PreviewChange) {
        let key = preview_row_key(&account_id, &change.id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow().get(&key).cloned() {
            row.state_label
                .set_label(preview_change_description(&change));
            row.state.set(change.state);
            self.reorder();
            return;
        }

        let change_id = change.id.clone();
        let (row, preview_row) = build_preview_row(change);
        if let Some(callback) = self.accept_preview.borrow().as_ref().cloned() {
            let account_id = account_id.clone();
            let change_id = change_id.clone();
            preview_row
                .accept_button
                .connect_clicked(move |_| callback(account_id.clone(), change_id.clone()));
        }
        if let Some(callback) = self.dismiss_preview.borrow().as_ref().cloned() {
            let account_id = account_id.clone();
            let change_id = change_id.clone();
            preview_row
                .dismiss_button
                .connect_clicked(move |_| callback(account_id.clone(), change_id.clone()));
        }
        let started_order = self.next_order.get();
        self.next_order.set(started_order.wrapping_add(1));
        preview_row.started_order.set(started_order);
        self.list.prepend(&row);
        self.rows
            .borrow_mut()
            .insert(key, ListRow::Preview(preview_row));
        self.reorder();
    }

    pub(in crate::app) fn mark_preview_applying(&self, account_id: &str, change_id: &str) {
        let key = preview_row_key(account_id, change_id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow().get(&key).cloned() {
            row.state.set(PreviewState::Applying);
            row.state_label.set_label("正在应用");
            row.accept_button.set_sensitive(false);
            row.dismiss_button.set_sensitive(false);
        }
    }

    pub(in crate::app) fn mark_preview_progress(
        &self,
        account_id: &str,
        change_id: &str,
        progress: f64,
    ) {
        let key = preview_row_key(account_id, change_id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow().get(&key).cloned() {
            row.state.set(PreviewState::Applying);
            row.state_label.set_label(&format!(
                "正在应用 {:.0}%",
                progress.clamp(0.0, 1.0) * 100.0
            ));
            row.progress_bar.set_visible(true);
            animate_preview_progress(&row, progress);
            row.accept_button.set_sensitive(false);
            row.dismiss_button.set_sensitive(false);
        }
    }

    pub(in crate::app) fn mark_preview_reconciling(&self, account_id: &str, change_id: &str) {
        let key = preview_row_key(account_id, change_id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow().get(&key).cloned() {
            row.state.set(PreviewState::Reconciling);
            row.state_label.set_label("正在更新同步状态");
            row.progress_bar.set_visible(false);
            row.accept_button.set_sensitive(false);
            row.dismiss_button.set_sensitive(false);
        }
    }

    pub(in crate::app) fn mark_preview_applied(&self, account_id: &str, change_id: &str) {
        let key = preview_row_key(account_id, change_id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow_mut().remove(&key) {
            self.list.remove(&row.list_row);
        }
    }

    pub(in crate::app) fn mark_preview_failed(
        &self,
        account_id: &str,
        change_id: &str,
        message: &str,
    ) {
        let key = preview_row_key(account_id, change_id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow().get(&key).cloned() {
            if matches!(row.state.get(), PreviewState::ReconcileFailed) {
                return;
            }
            row.state.set(PreviewState::Failed);
            row.state_label.set_label(message);
            row.progress_bar.set_visible(false);
            row.accept_button.set_sensitive(true);
            row.dismiss_button.set_sensitive(true);
        }
    }

    pub(in crate::app) fn mark_preview_reconcile_failed(
        &self,
        account_id: &str,
        change_id: &str,
        message: &str,
    ) {
        let key = preview_row_key(account_id, change_id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow().get(&key).cloned() {
            row.state.set(PreviewState::ReconcileFailed);
            row.state_label.set_label(message);
            row.progress_bar.set_visible(false);
            row.accept_button.set_sensitive(true);
            row.dismiss_button.set_sensitive(true);
        }
    }

    pub(in crate::app) fn dismiss_preview(&self, account_id: &str, change_id: &str) {
        let key = preview_row_key(account_id, change_id);
        if let Some(ListRow::Preview(row)) = self.rows.borrow_mut().remove(&key) {
            self.list.remove(&row.list_row);
        }
    }

    pub(in crate::app) fn connect_preview_accept<F>(&self, callback: F)
    where
        F: Fn(String, String) + 'static,
    {
        self.accept_preview.replace(Some(Rc::new(callback)));
    }

    pub(in crate::app) fn connect_preview_dismiss<F>(&self, callback: F)
    where
        F: Fn(String, String) + 'static,
    {
        self.dismiss_preview.replace(Some(Rc::new(callback)));
    }

    fn reorder(&self) {
        let mut rows: Vec<ListRow> = self.rows.borrow().values().cloned().collect();
        rows.sort_by_key(|row| match row {
            ListRow::Live(row) => (row.completed.get(), Reverse(row.started_order.get())),
            ListRow::Preview(row) => (false, Reverse(row.started_order.get())),
        });

        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for row in rows {
            match row {
                ListRow::Live(row) => self.list.append(&row.list_row),
                ListRow::Preview(row) => self.list.append(&row.list_row),
            }
        }
    }
}

fn preview_row_key(account_id: &str, change_id: &str) -> String {
    format!("preview:{account_id}:{change_id}")
}

fn build_file_row(file: FileChange) -> (gtk::ListBoxRow, TransferRow) {
    let list_row = gtk::ListBoxRow::new();

    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();

    let icon = gtk::Image::from_icon_name(change_kind_icon(file.kind));
    icon.set_valign(Align::Center);

    let text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    text_box.set_size_request(FILE_COLUMN_WIDTH, -1);

    let name = gtk::Label::builder()
        .label(&file.name)
        .halign(Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let state = gtk::Label::builder()
        .label(file_display_state(&file))
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
    let transfer_list_row = list_row.clone();

    (
        list_row,
        TransferRow {
            list_row: transfer_list_row,
            state_label: state,
            progress_bar: progress,
            progress_animation: Rc::new(Cell::new(0)),
            started_order: Cell::new(0),
            target_progress: Cell::new(file.progress),
            completed: Cell::new(file.progress >= 1.0),
        },
    )
}

fn build_preview_row(change: PreviewChange) -> (gtk::ListBoxRow, PreviewRow) {
    let list_row = gtk::ListBoxRow::new();
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();

    let icon = gtk::Image::from_icon_name(change_kind_icon(change.kind));
    icon.set_valign(Align::Center);

    let text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    text_box.set_size_request(FILE_COLUMN_WIDTH, -1);

    let name = gtk::Label::builder()
        .label(&change.path)
        .halign(Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let state_text = format!(
        "{} · {}",
        preview_intent_label(change.intent),
        preview_change_description(&change),
    );
    let state = gtk::Label::builder()
        .label(state_text)
        .halign(Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["dim-label"])
        .build();
    state.set_tooltip_text(Some(preview_intent_detail(change.intent)));
    text_box.append(&name);
    text_box.append(&state);

    let progress = gtk::ProgressBar::builder()
        .fraction(0.0)
        .valign(Align::Center)
        .width_request(FILE_COLUMN_WIDTH)
        .visible(false)
        .build();
    progress.set_size_request(FILE_COLUMN_WIDTH, -1);
    progress.set_hexpand(false);

    let accept_button = gtk::Button::builder()
        .icon_name("object-select-symbolic")
        .tooltip_text("应用该变更")
        .css_classes(["flat"])
        .build();
    accept_button.set_widget_name(&change.id);

    let dismiss_button = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("放弃该变更")
        .css_classes(["flat"])
        .build();
    dismiss_button.set_widget_name(&change.id);

    row_box.append(&icon);
    row_box.append(&text_box);
    row_box.append(&progress);
    row_box.append(&accept_button);
    row_box.append(&dismiss_button);
    list_row.set_child(Some(&row_box));

    (
        list_row.clone(),
        PreviewRow {
            list_row,
            state_label: state,
            progress_bar: progress,
            progress_animation: Rc::new(Cell::new(0)),
            accept_button,
            dismiss_button,
            started_order: Cell::new(0),
            state: Cell::new(change.state),
        },
    )
}

fn animate(bar: &gtk::ProgressBar, counter: &Rc<Cell<u64>>, target: f64) {
    let target = target.clamp(0.0, 1.0);
    let start = bar.fraction();
    let animation = counter.get().wrapping_add(1);
    counter.set(animation);

    if (target - start).abs() < 0.001 {
        bar.set_fraction(target);
        return;
    }

    let progress_bar = bar.clone();
    let progress_animation = Rc::clone(counter);
    let started_at = Instant::now();

    gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
        if progress_animation.get() != animation {
            return gtk::glib::ControlFlow::Break;
        }

        let elapsed = started_at.elapsed().as_secs_f64();
        let duration = Duration::from_millis(PROGRESS_ANIMATION_MS).as_secs_f64();
        let progress = (elapsed / duration).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(2);
        progress_bar.set_fraction(start + ((target - start) * eased));

        if progress >= 1.0 {
            progress_bar.set_fraction(target);
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn animate_progress(row: &TransferRow, target: f64) {
    animate(&row.progress_bar, &row.progress_animation, target);
}

fn animate_preview_progress(row: &PreviewRow, target: f64) {
    animate(&row.progress_bar, &row.progress_animation, target);
}
