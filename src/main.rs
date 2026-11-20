mod app;
pub mod buffer;
mod dired;
mod editor;
mod ex;
mod input;
mod minibuffer;
mod navigation;
mod output;
mod plugin;
mod search;
mod shell;
mod syntax;
mod ui;
pub mod vim;

fn main() {
    app::run();
}
