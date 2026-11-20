use gpui::{Div, Window, div, prelude::*, px, rgb};

use crate::{editor::Editor, vim::Mode};

pub const LINE_HEIGHT: gpui::Pixels = px(21.0);
pub const CHROME_HEIGHT: gpui::Pixels = px(56.0);
const GUTTER_WIDTH: gpui::Pixels = px(64.0);
const APPROXIMATE_CHARACTER_WIDTH: gpui::Pixels = px(8.5);

const BACKGROUND: u32 = 0x181818;
const SURFACE: u32 = 0x202020;
const STATUS_BACKGROUND: u32 = 0x2a2a2a;
const TEXT: u32 = 0xd4d4d4;
const MUTED_TEXT: u32 = 0x707070;
const ACCENT: u32 = 0x5b8bd9;
const CURSOR_TEXT: u32 = 0x181818;
const SELECTION_BACKGROUND: u32 = 0x264f78;

pub fn visible_geometry(window: &Window) -> (usize, usize) {
    let viewport = window.viewport_size();
    let rows = ((viewport.height - CHROME_HEIGHT) / LINE_HEIGHT).floor() as usize;
    let columns = ((viewport.width - GUTTER_WIDTH) / APPROXIMATE_CHARACTER_WIDTH).floor() as usize;
    (rows.max(1), columns.max(1))
}

pub fn render(editor: &Editor, focused: bool, visible_rows: usize) -> Div {
    let cursor = editor.cursor().position;
    let viewport = editor.viewport();
    let last_visible_line = (viewport.top_line + visible_rows + 1).min(editor.buffer().len_lines());

    let rows = (viewport.top_line..last_visible_line).map(|line_index| {
        render_line(
            editor,
            line_index,
            cursor.line,
            cursor.character,
            editor.mode(),
        )
    });

    let modified = if !editor.is_dired() && !editor.is_output() && editor.buffer().is_modified() {
        " [+]"
    } else {
        ""
    };
    let focus_status = if focused { "FOCUSED" } else { "UNFOCUSED" };
    let mode = if editor.is_dired() {
        "DIRED"
    } else if editor.is_output() {
        "OUTPUT"
    } else {
        editor.mode().label()
    };
    let minibuffer = editor.prompt_text().map_or_else(
        || div().child(editor.message().to_owned()),
        |prompt| {
            div()
                .flex()
                .w_full()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(prompt)
                        .child(div().w(px(2.0)).h(px(18.0)).flex_shrink_0().bg(rgb(ACCENT))),
                )
                .child(
                    div()
                        .text_color(rgb(MUTED_TEXT))
                        .child(editor.message().to_owned()),
                )
        },
    );

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgb(BACKGROUND))
        .text_color(rgb(TEXT))
        .font_family("JetBrains Mono")
        .text_size(px(14.0))
        .child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .flex_col()
                .overflow_hidden()
                .children(rows),
        )
        .child(
            div()
                .flex()
                .h(px(28.0))
                .flex_shrink_0()
                .items_center()
                .justify_between()
                .px_2()
                .bg(rgb(STATUS_BACKGROUND))
                .child(mode)
                .child(format!("{}{}", editor.display_name(), modified))
                .child(format!(
                    "{}  {}:{}",
                    focus_status,
                    cursor.line + 1,
                    cursor.character + 1
                )),
        )
        .child(
            div()
                .flex()
                .h(px(28.0))
                .flex_shrink_0()
                .items_center()
                .px_2()
                .bg(rgb(SURFACE))
                .child(minibuffer),
        )
}

fn render_line(
    editor: &Editor,
    line_index: usize,
    cursor_line: usize,
    cursor_character: usize,
    mode: Mode,
) -> Div {
    let number = displayed_line_number(line_index, cursor_line);
    let text = editor.buffer().line_text(line_index).unwrap_or_default();
    let left = editor.viewport().left_character.min(text.chars().count());

    let content = render_text_line(
        &text,
        left,
        (line_index == cursor_line).then_some(cursor_character),
        mode,
        editor.selection_columns(line_index),
    );

    div()
        .flex()
        .h(LINE_HEIGHT)
        .flex_shrink_0()
        .line_height(LINE_HEIGHT)
        .whitespace_nowrap()
        .child(
            div()
                .flex()
                .w(GUTTER_WIDTH)
                .flex_shrink_0()
                .justify_end()
                .pr_3()
                .text_color(if line_index == cursor_line {
                    rgb(TEXT)
                } else {
                    rgb(MUTED_TEXT)
                })
                .child(number.to_string()),
        )
        .child(div().flex_1().min_w_0().overflow_hidden().child(content))
}

fn displayed_line_number(line_index: usize, cursor_line: usize) -> usize {
    if line_index == cursor_line {
        line_index + 1
    } else {
        line_index.abs_diff(cursor_line)
    }
}

fn render_text_line(
    text: &str,
    left: usize,
    cursor_character: Option<usize>,
    mode: Mode,
    selection: Option<(usize, usize)>,
) -> Div {
    let characters: Vec<char> = text.chars().collect();
    let cursor = cursor_character.map(|cursor| cursor.min(characters.len()));
    if mode == Mode::Insert
        && let Some(cursor) = cursor
    {
        let visible_start = left.min(cursor);
        let before: String = characters[visible_start..cursor].iter().collect();
        let after: String = characters[cursor..].iter().collect();
        return div()
            .flex()
            .whitespace_nowrap()
            .child(before)
            .child(
                div()
                    .w(px(2.0))
                    .h(LINE_HEIGHT)
                    .flex_shrink_0()
                    .bg(rgb(ACCENT)),
            )
            .child(after);
    }

    let mut line = div().flex().whitespace_nowrap();
    for (index, character) in characters.iter().enumerate().skip(left) {
        let selected = selection.is_some_and(|(start, end)| index >= start && index <= end);
        let is_cursor = cursor == Some(index);
        let mut cell = div().child(character.to_string());
        if is_cursor {
            cell = cell.bg(rgb(ACCENT)).text_color(rgb(CURSOR_TEXT));
        } else if selected {
            cell = cell.bg(rgb(SELECTION_BACKGROUND));
        }
        line = line.child(cell);
    }
    if cursor == Some(characters.len()) || (characters.is_empty() && selection.is_some()) {
        let mut cell = div().child(" ");
        if cursor == Some(characters.len()) {
            cell = cell.bg(rgb(ACCENT)).text_color(rgb(CURSOR_TEXT));
        } else {
            cell = cell.bg(rgb(SELECTION_BACKGROUND));
        }
        line = line.child(cell);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::displayed_line_number;

    #[test]
    fn cursor_line_is_absolute_and_other_lines_are_relative() {
        assert_eq!(displayed_line_number(4, 4), 5);
        assert_eq!(displayed_line_number(1, 4), 3);
        assert_eq!(displayed_line_number(7, 4), 3);
    }
}
