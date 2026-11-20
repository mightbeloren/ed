use std::path::Path;

use crate::{
    buffer::{Buffer, BufferPosition},
    input::{Key, KeyValue},
};

const DEFAULT_VISIBLE_ROWS: usize = 24;
const DEFAULT_VISIBLE_COLUMNS: usize = 80;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub position: BufferPosition,
    desired_character: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Viewport {
    pub top_line: usize,
    pub left_character: usize,
}

pub struct Editor {
    buffer: Buffer,
    cursor: Cursor,
    viewport: Viewport,
    visible_rows: usize,
    visible_columns: usize,
    message: String,
}

impl Editor {
    pub fn scratch() -> Self {
        Self::with_buffer(Buffer::scratch(), String::new())
    }

    pub fn open(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        match Buffer::open(path) {
            Ok(buffer) => Self::with_buffer(buffer, String::new()),
            Err(error) => Self::with_buffer(
                Buffer::scratch(),
                format!("Could not open {}: {error}", path.display()),
            ),
        }
    }

    fn with_buffer(buffer: Buffer, message: String) -> Self {
        Self {
            buffer,
            cursor: Cursor::default(),
            viewport: Viewport::default(),
            visible_rows: DEFAULT_VISIBLE_ROWS,
            visible_columns: DEFAULT_VISIBLE_COLUMNS,
            message,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn set_visible_geometry(&mut self, rows: usize, columns: usize) {
        self.visible_rows = rows.max(1);
        self.visible_columns = columns.max(1);
        self.ensure_cursor_visible();
    }

    pub fn handle_key(&mut self, key: &Key) {
        match (&key.value, key.modifiers.control) {
            (KeyValue::Named(name), true) if name == "s" => self.save(),
            (KeyValue::Named(name), false) if name == "up" => self.move_vertical(-1),
            (KeyValue::Named(name), false) if name == "down" => self.move_vertical(1),
            (KeyValue::Named(name), false) if name == "left" => self.move_horizontal(-1),
            (KeyValue::Named(name), false) if name == "right" => self.move_horizontal(1),
            (KeyValue::Named(name), false) if name == "home" => self.move_to_line_boundary(false),
            (KeyValue::Named(name), false) if name == "end" => self.move_to_line_boundary(true),
            (KeyValue::Named(name), false) if name == "pageup" => {
                self.move_vertical(-(self.visible_rows as isize))
            }
            (KeyValue::Named(name), false) if name == "pagedown" => {
                self.move_vertical(self.visible_rows as isize)
            }
            _ => return,
        }
        self.message.clear();
    }

    fn save(&mut self) {
        self.message = match self.buffer.save() {
            Ok(()) => format!("Wrote {}", self.buffer.display_name()),
            Err(error) => format!("Write failed: {error}"),
        };
    }

    fn move_vertical(&mut self, delta: isize) {
        let last_line = self.buffer.len_lines().saturating_sub(1);
        let line = self
            .cursor
            .position
            .line
            .saturating_add_signed(delta)
            .min(last_line);
        let desired = self
            .cursor
            .desired_character
            .unwrap_or(self.cursor.position.character);
        let character = desired.min(self.buffer.line_len_chars(line).unwrap_or(0));

        self.cursor.position = BufferPosition { line, character };
        self.cursor.desired_character = Some(desired);
        self.ensure_cursor_visible();
    }

    fn move_horizontal(&mut self, delta: isize) {
        let line_length = self
            .buffer
            .line_len_chars(self.cursor.position.line)
            .unwrap_or(0);
        self.cursor.position.character = self
            .cursor
            .position
            .character
            .saturating_add_signed(delta)
            .min(line_length);
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn move_to_line_boundary(&mut self, end: bool) {
        self.cursor.position.character = if end {
            self.buffer
                .line_len_chars(self.cursor.position.line)
                .unwrap_or(0)
        } else {
            0
        };
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn ensure_cursor_visible(&mut self) {
        let cursor_line = self.cursor.position.line;
        if cursor_line < self.viewport.top_line {
            self.viewport.top_line = cursor_line;
        } else if cursor_line >= self.viewport.top_line + self.visible_rows {
            self.viewport.top_line = cursor_line + 1 - self.visible_rows;
        }

        let cursor_character = self.cursor.position.character;
        if cursor_character < self.viewport.left_character {
            self.viewport.left_character = cursor_character;
        } else if cursor_character >= self.viewport.left_character + self.visible_columns {
            self.viewport.left_character = cursor_character + 1 - self.visible_columns;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::{Key, KeyValue, Modifiers};

    use super::*;

    fn named(name: &str) -> Key {
        Key {
            value: KeyValue::Named(name.to_owned()),
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn vertical_movement_preserves_desired_character() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(crate::buffer::CharOffset(0), "12345\n1\n12345")
            .expect("valid insertion");
        editor.cursor.position.character = 4;

        editor.handle_key(&named("down"));
        assert_eq!(editor.cursor.position.character, 1);

        editor.handle_key(&named("down"));
        assert_eq!(editor.cursor.position.character, 4);
    }

    #[test]
    fn scrolling_keeps_cursor_inside_viewport() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(crate::buffer::CharOffset(0), "1\n2\n3\n4\n5")
            .expect("valid insertion");
        editor.set_visible_geometry(2, 80);

        editor.handle_key(&named("pagedown"));

        assert_eq!(editor.cursor.position.line, 2);
        assert_eq!(editor.viewport.top_line, 1);
    }
}
