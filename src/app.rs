use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

use crate::input::Key;

const BACKGROUND: u32 = 0x1e1e2e;
const SURFACE: u32 = 0x313244;
const STATUS_BACKGROUND: u32 = 0x45475a;
const TEXT: u32 = 0xcdd6f4;
const MUTED_TEXT: u32 = 0xa6adc8;
const ACCENT: u32 = 0x89b4fa;

pub struct EditorApp {
    focus_handle: FocusHandle,
    last_key: Option<Key>,
}

impl EditorApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);

        Self {
            focus_handle,
            last_key: None,
        }
    }

    fn record_key(&mut self, key: Key, cx: &mut Context<Self>) {
        self.last_key = Some(key);
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
        let focused = self.focus_handle.is_focused(window);
        let key_text = self
            .last_key
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "press any key".to_owned());

        div()
            .track_focus(&self.focus_handle(cx))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BACKGROUND))
            .text_color(rgb(TEXT))
            .font_family("monospace")
            .child(
                div()
                    .flex()
                    .flex_1()
                    .flex_col()
                    .justify_center()
                    .items_center()
                    .gap_2()
                    .child(div().text_xl().child("ed"))
                    .child(
                        div()
                            .text_color(rgb(MUTED_TEXT))
                            .child("Phase 0 — editor viewport placeholder"),
                    )
                    .child(
                        div()
                            .text_color(rgb(ACCENT))
                            .child(format!("Key: {key_text}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .h(px(28.0))
                    .items_center()
                    .justify_between()
                    .px_2()
                    .bg(rgb(STATUS_BACKGROUND))
                    .child("NORMAL")
                    .child("[No Name]   1:1")
                    .child(if focused { "FOCUSED" } else { "UNFOCUSED" }),
            )
            .child(
                div()
                    .flex()
                    .h(px(28.0))
                    .items_center()
                    .px_2()
                    .bg(rgb(SURFACE))
                    .child("minibuffer"),
            )
    }
}

pub fn run() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.0), px(700.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| EditorApp::new(window, cx)),
            )
            .expect("failed to open ed window");

        let editor = window
            .update(cx, |_editor, _window, cx| cx.entity())
            .expect("ed window closed during startup");

        cx.observe_keystrokes(move |event, _window, cx| {
            editor.update(cx, |editor, cx| {
                editor.record_key(Key::from(&event.keystroke), cx);
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
