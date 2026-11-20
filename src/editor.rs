use std::path::Path;

use crate::{
    buffer::{Buffer, BufferPosition, CharOffset},
    input::{Key, KeyValue},
    vim::{Command, InsertPlacement, Mode, Motion, VimEngine},
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
    vim: VimEngine,
    insert_session_changed: bool,
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
            vim: VimEngine::default(),
            insert_session_changed: false,
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

    pub fn mode(&self) -> Mode {
        self.vim.mode()
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
        if matches!(&key.value, KeyValue::Named(name) if name == "s") && key.modifiers.control {
            self.save();
            return;
        }

        match self.vim.mode() {
            Mode::Normal => {
                if let Some(command) = self.vim.normal_command(key) {
                    self.execute_normal(command);
                    self.message.clear();
                }
            }
            Mode::Insert => self.handle_insert_key(key),
        }
    }

    fn execute_normal(&mut self, command: Command) {
        match command {
            Command::Move { motion, count } => self.execute_motion(motion, count),
            Command::GoToLine { line } => {
                let line = line.unwrap_or_else(|| self.buffer.len_lines().saturating_sub(1));
                self.move_to_line(line);
            }
            Command::EnterInsert(placement) => self.enter_insert(placement),
        }
    }

    fn execute_motion(&mut self, motion: Motion, count: u32) {
        let count = count as usize;
        match motion {
            Motion::Left => self.move_horizontal(-(count.min(isize::MAX as usize) as isize)),
            Motion::Down => self.move_vertical(count.min(isize::MAX as usize) as isize),
            Motion::Up => self.move_vertical(-(count.min(isize::MAX as usize) as isize)),
            Motion::Right => self.move_horizontal(count.min(isize::MAX as usize) as isize),
            Motion::LineStart => self.move_to_character(0),
            Motion::FirstNonBlank => self.move_to_first_non_blank(),
            Motion::LineEnd => {
                if count > 1 {
                    self.move_vertical((count - 1).min(isize::MAX as usize) as isize);
                }
                self.move_to_line_end();
            }
            Motion::PageUp => {
                let distance = count
                    .saturating_mul(self.visible_rows)
                    .min(isize::MAX as usize);
                self.move_vertical(-(distance as isize));
            }
            Motion::PageDown => {
                let distance = count
                    .saturating_mul(self.visible_rows)
                    .min(isize::MAX as usize);
                self.move_vertical(distance as isize);
            }
        }
    }

    fn enter_insert(&mut self, placement: InsertPlacement) {
        self.buffer.begin_transaction();
        self.insert_session_changed = false;

        match placement {
            InsertPlacement::BeforeCursor => {}
            InsertPlacement::AfterCursor => {
                let line_length = self.current_line_length();
                self.cursor.position.character = self
                    .cursor
                    .position
                    .character
                    .saturating_add(1)
                    .min(line_length);
            }
            InsertPlacement::FirstNonBlank => self.move_to_first_non_blank(),
            InsertPlacement::EndOfLine => {
                self.cursor.position.character = self.current_line_length();
            }
            InsertPlacement::OpenBelow => {
                self.cursor.position.character = self.current_line_length();
                self.insert_text("\n");
            }
            InsertPlacement::OpenAbove => {
                self.cursor.position.character = 0;
                self.insert_text("\n");
                self.cursor.position.line = self.cursor.position.line.saturating_sub(1);
                self.cursor.position.character = 0;
            }
        }

        self.cursor.desired_character = None;
        self.vim.enter_insert();
        self.ensure_cursor_visible();
    }

    fn handle_insert_key(&mut self, key: &Key) {
        match &key.value {
            KeyValue::Named(name) if name == "escape" => self.leave_insert(),
            KeyValue::Named(name) if name == "backspace" => self.backspace(),
            KeyValue::Named(name) if name == "enter" => self.insert_text("\n"),
            KeyValue::Named(name) if name == "left" => self.move_horizontal(-1),
            KeyValue::Named(name) if name == "right" => self.move_horizontal(1),
            KeyValue::Named(name) if name == "up" => self.move_vertical(-1),
            KeyValue::Named(name) if name == "down" => self.move_vertical(1),
            KeyValue::Named(name) if name == "home" => self.move_to_character(0),
            KeyValue::Named(name) if name == "end" => {
                self.cursor.position.character = self.current_line_length();
                self.ensure_cursor_visible();
            }
            KeyValue::Character(text)
                if !key.modifiers.control && !key.modifiers.alt && !key.modifiers.super_key =>
            {
                self.insert_text(text)
            }
            _ => return,
        }
        self.message.clear();
    }

    fn leave_insert(&mut self) {
        self.buffer.commit_transaction();
        self.vim.leave_insert();
        if self.insert_session_changed && self.cursor.position.character > 0 {
            self.cursor.position.character -= 1;
        }
        self.clamp_cursor_to_mode();
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn insert_text(&mut self, text: &str) {
        let Ok(offset) = self.buffer.position_to_offset(self.cursor.position) else {
            return;
        };
        if self.buffer.insert(offset, text).is_err() {
            return;
        }
        let new_offset = CharOffset(offset.0 + text.chars().count());
        if let Ok(position) = self.buffer.offset_to_position(new_offset) {
            self.cursor.position = position;
        }
        self.insert_session_changed |= !text.is_empty();
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn backspace(&mut self) {
        let Ok(offset) = self.buffer.position_to_offset(self.cursor.position) else {
            return;
        };
        if offset.0 == 0 {
            return;
        }

        let mut start = CharOffset(offset.0 - 1);
        if self.buffer.char_at(start) == Some('\n')
            && start.0 > 0
            && self.buffer.char_at(CharOffset(start.0 - 1)) == Some('\r')
        {
            start.0 -= 1;
        }
        if self.buffer.remove(start, offset).is_err() {
            return;
        }
        if let Ok(position) = self.buffer.offset_to_position(start) {
            self.cursor.position = position;
        }
        self.insert_session_changed = true;
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
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
        let character = desired.min(self.maximum_character(line));

        self.cursor.position = BufferPosition { line, character };
        self.cursor.desired_character = Some(desired);
        self.ensure_cursor_visible();
    }

    fn move_horizontal(&mut self, delta: isize) {
        let maximum = self.maximum_character(self.cursor.position.line);
        self.cursor.position.character = self
            .cursor
            .position
            .character
            .saturating_add_signed(delta)
            .min(maximum);
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn move_to_line(&mut self, line: usize) {
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        let desired = self
            .cursor
            .desired_character
            .unwrap_or(self.cursor.position.character);
        self.cursor.position = BufferPosition {
            line,
            character: desired.min(self.maximum_character(line)),
        };
        self.cursor.desired_character = Some(desired);
        self.ensure_cursor_visible();
    }

    fn move_to_character(&mut self, character: usize) {
        self.cursor.position.character =
            character.min(self.maximum_character(self.cursor.position.line));
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn move_to_first_non_blank(&mut self) {
        let character = self
            .buffer
            .line_text(self.cursor.position.line)
            .unwrap_or_default()
            .chars()
            .take_while(|character| character.is_whitespace())
            .count();
        self.move_to_character(character);
    }

    fn move_to_line_end(&mut self) {
        let line_length = self.current_line_length();
        self.cursor.position.character = if self.vim.mode() == Mode::Insert {
            line_length
        } else {
            line_length.saturating_sub(1)
        };
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn current_line_length(&self) -> usize {
        self.buffer
            .line_len_chars(self.cursor.position.line)
            .unwrap_or(0)
    }

    fn maximum_character(&self, line: usize) -> usize {
        let line_length = self.buffer.line_len_chars(line).unwrap_or(0);
        if self.vim.mode() == Mode::Insert {
            line_length
        } else {
            line_length.saturating_sub(1)
        }
    }

    fn clamp_cursor_to_mode(&mut self) {
        self.cursor.position.line = self
            .cursor
            .position
            .line
            .min(self.buffer.len_lines().saturating_sub(1));
        self.cursor.position.character = self
            .cursor
            .position
            .character
            .min(self.maximum_character(self.cursor.position.line));
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

    fn character(value: char) -> Key {
        Key {
            value: KeyValue::Character(value.to_string()),
            modifiers: Modifiers::default(),
        }
    }

    fn named(name: &str) -> Key {
        Key {
            value: KeyValue::Named(name.to_owned()),
            modifiers: Modifiers::default(),
        }
    }

    fn type_keys(editor: &mut Editor, keys: &str) {
        for value in keys.chars() {
            editor.handle_key(&character(value));
        }
    }

    #[test]
    fn normal_mode_moves_with_counts_and_preserves_desired_column() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "12345\n1\n12345")
            .expect("valid insertion");
        editor.cursor.position.character = 4;

        type_keys(&mut editor, "j");
        assert_eq!(editor.cursor.position.character, 0);

        type_keys(&mut editor, "j");
        assert_eq!(editor.cursor.position.character, 4);

        type_keys(&mut editor, "2k");
        assert_eq!(editor.cursor.position.line, 0);
    }

    #[test]
    fn scrolling_keeps_vim_cursor_inside_viewport() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "1\n2\n3\n4\n5")
            .expect("valid insertion");
        editor.set_visible_geometry(2, 80);

        type_keys(&mut editor, "3j");

        assert_eq!(editor.cursor.position.line, 3);
        assert_eq!(editor.viewport.top_line, 2);
    }

    #[test]
    fn insert_session_is_one_transaction() {
        let mut editor = Editor::scratch();

        type_keys(&mut editor, "ihello");
        editor.handle_key(&named("escape"));

        assert_eq!(editor.mode(), Mode::Normal);
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("hello"));
        assert_eq!(editor.cursor.position.character, 4);
        assert_eq!(editor.buffer.completed_transaction_count(), 1);
    }

    #[test]
    fn append_and_linewise_insert_placements_match_vim_positions() {
        let mut append = Editor::scratch();
        append
            .buffer
            .insert(CharOffset(0), "abc")
            .expect("valid insertion");
        type_keys(&mut append, "aX");
        append.handle_key(&named("escape"));
        assert_eq!(append.buffer.line_text(0).as_deref(), Some("aXbc"));

        let mut line_start = Editor::scratch();
        line_start
            .buffer
            .insert(CharOffset(0), "  abc")
            .expect("valid insertion");
        type_keys(&mut line_start, "I>");
        line_start.handle_key(&named("escape"));
        assert_eq!(line_start.buffer.line_text(0).as_deref(), Some("  >abc"));

        let mut line_end = Editor::scratch();
        line_end
            .buffer
            .insert(CharOffset(0), "abc")
            .expect("valid insertion");
        type_keys(&mut line_end, "A!");
        line_end.handle_key(&named("escape"));
        assert_eq!(line_end.buffer.line_text(0).as_deref(), Some("abc!"));
    }

    #[test]
    fn enter_splits_a_line_during_insert_mode() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "ab")
            .expect("valid insertion");
        type_keys(&mut editor, "li");
        editor.handle_key(&named("enter"));
        type_keys(&mut editor, "x");
        editor.handle_key(&named("escape"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("a"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("xb"));
        assert_eq!(editor.buffer.completed_transaction_count(), 1);
    }

    #[test]
    fn open_below_enters_insert_on_a_new_line() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "first\nsecond")
            .expect("valid insertion");

        type_keys(&mut editor, "oadded");
        editor.handle_key(&named("escape"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("first"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("added"));
        assert_eq!(editor.buffer.line_text(2).as_deref(), Some("second"));
        assert_eq!(editor.buffer.completed_transaction_count(), 1);
    }

    #[test]
    fn insert_backspace_joins_lines() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "one\ntwo")
            .expect("valid insertion");
        editor.cursor.position = BufferPosition {
            line: 1,
            character: 0,
        };

        type_keys(&mut editor, "i");
        editor.handle_key(&named("backspace"));
        editor.handle_key(&named("escape"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("onetwo"));
        assert_eq!(editor.buffer.len_lines(), 1);
    }

    #[test]
    fn jumps_to_first_and_last_lines() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "one\ntwo\nthree")
            .expect("valid insertion");

        type_keys(&mut editor, "G");
        assert_eq!(editor.cursor.position.line, 2);
        type_keys(&mut editor, "gg");
        assert_eq!(editor.cursor.position.line, 0);
        type_keys(&mut editor, "2G");
        assert_eq!(editor.cursor.position.line, 1);
    }

    #[test]
    fn counted_line_end_moves_across_lines() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "one\nsecond\nthree")
            .expect("valid insertion");

        type_keys(&mut editor, "2$");

        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 1,
                character: 5,
            }
        );
    }

    #[test]
    fn open_above_inserts_before_the_current_line() {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), "first\nsecond")
            .expect("valid insertion");
        type_keys(&mut editor, "jOadded");
        editor.handle_key(&named("escape"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("first"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("added"));
        assert_eq!(editor.buffer.line_text(2).as_deref(), Some("second"));
    }
}
