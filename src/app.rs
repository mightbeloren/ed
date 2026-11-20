use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, ClipboardItem, Context, FocusHandle, Focusable, Window, WindowBounds,
    WindowOptions, prelude::*, px, size,
};

use crate::{
    editor::{Editor, EditorRequest},
    input::Key,
    shell::{self, ShellRequest},
    ui,
};

pub struct EditorApp {
    focus_handle: FocusHandle,
    editor: Editor,
}

impl EditorApp {
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);
        let editor = path.map(Editor::open).unwrap_or_else(Editor::dired_current);

        Self {
            focus_handle,
            editor,
        }
    }

    fn handle_key(&mut self, key: Key, cx: &mut Context<Self>) -> Option<EditorRequest> {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.editor.set_system_clipboard(text);
        }
        self.editor.handle_key(&key);
        if let Some(text) = self.editor.take_system_clipboard_write() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        let request = match self.editor.take_request() {
            Some(EditorRequest::RunShell {
                request,
                open_immediately,
            }) => {
                Self::run_shell(request, open_immediately, cx);
                None
            }
            request => request,
        };
        cx.notify();
        request
    }

    fn run_shell(request: ShellRequest, open_immediately: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let completion = cx
                .background_executor()
                .spawn(async move { shell::run(request) })
                .await;
            let _ = this.update(cx, move |app, cx| {
                app.editor.finish_shell(completion, open_immediately);
                cx.notify();
            });
        })
        .detach();
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

        cx.observe_keystrokes(move |event, window, cx| {
            let request = editor.update(cx, |editor, cx| {
                editor.handle_key(Key::from(&event.keystroke), cx)
            });
            if matches!(request, Some(EditorRequest::Close)) {
                window.remove_window();
            }
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
