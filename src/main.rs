mod app;
pub mod buffer;
mod editor;
mod ex;
mod input;
mod minibuffer;
mod navigation;
mod search;
mod ui;
pub mod vim;

fn main() {
    app::run();
}
