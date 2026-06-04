mod account;
mod app;
mod config;
mod onedrive;
mod settings;
mod sync;
mod transfer;
mod utils;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
