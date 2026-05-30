mod account;
mod app;
mod config;
mod onedrive;
mod path;
mod settings;
mod transfer;
mod ui;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
