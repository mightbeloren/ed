use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, Window, WindowBounds, WindowOptions,
    prelude::*, px, size,
};

use crate::{editor::Editor, input::Key, ui};

pub struct EditorApp {
    focus_handle: FocusHandle,
    editor: Editor,
}

impl EditorApp {
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);
        let editor = path.map(Editor::open).unwrap_or_else(Editor::scratch);

        Self {
            focus_handle,
            editor,
        }
    }

    fn handle_key(&mut self, key: Key, cx: &mut Context<Self>) {
        self.editor.handle_key(&key);
        cx.notify();
    }
}

impl Focusable for EditorApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (rows, columns) = ui::visible_geometry(window);
        self.editor.set_visible_geometry(rows, columns);

        ui::render(&self.editor, self.focus_handle.is_focused(window), rows)
            .track_focus(&self.focus_handle(cx))
    }
}

pub fn run() {
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.0), px(700.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                {
                    let path = path.clone();
                    move |window, cx| cx.new(|cx| EditorApp::new(path, window, cx))
                },
            )
            .expect("failed to open ed window");

        let editor = window
            .update(cx, |_editor, _window, cx| cx.entity())
            .expect("ed window closed during startup");

        cx.observe_keystrokes(move |event, _window, cx| {
            editor.update(cx, |editor, cx| {
                editor.handle_key(Key::from(&event.keystroke), cx);
            });
        })
        .detach();

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.activate(true);
    });
}
