mod app;
pub mod buffer;
mod editor;
mod input;
mod navigation;
mod search;
mod ui;
pub mod vim;

fn main() {
    app::run();
}
