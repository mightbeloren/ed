use std::path::Path;

use crate::{
    buffer::{Buffer, BufferPosition, CharOffset},
    input::{Key, KeyValue},
    vim::{
        Command, FindMotion, InsertPlacement, Mode, Motion, Operator, OperatorCommand,
        RegisterName, RegisterStore, RegisterValue, ResolvedTarget, SelectionShape, Target,
        VimEngine, VisualKind, resolve_motion, resolve_target,
    },
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

#[derive(Clone, Debug)]
struct BlockInsertState {
    lines: Vec<usize>,
    primary_line: usize,
    column: usize,
    inserted_text: String,
}

pub struct Editor {
    buffer: Buffer,
    cursor: Cursor,
    viewport: Viewport,
    vim: VimEngine,
    registers: RegisterStore,
    last_find: Option<FindMotion>,
    visual_anchor: Option<BufferPosition>,
    block_insert: Option<BlockInsertState>,
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
            registers: RegisterStore::default(),
            last_find: None,
            visual_anchor: None,
            block_insert: None,
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

    pub fn selection_columns(&self, line: usize) -> Option<(usize, usize)> {
        let anchor = self.visual_anchor?;
        let kind = self.vim.visual_kind()?;
        let head = self.cursor.position;
        let first_line = anchor.line.min(head.line);
        let last_line = anchor.line.max(head.line);
        if line < first_line || line > last_line {
            return None;
        }
        let line_length = self.buffer.line_len_chars(line).unwrap_or(0);
        let range = match kind {
            VisualKind::Line => (0, line_length.saturating_sub(1)),
            VisualKind::Block => {
                let start = anchor.character.min(head.character).min(line_length);
                let end = anchor
                    .character
                    .max(head.character)
                    .min(line_length.saturating_sub(1));
                (start, end)
            }
            VisualKind::Character => {
                let (start_position, end_position) = ordered_positions(anchor, head);
                let start = if line == start_position.line {
                    start_position.character
                } else {
                    0
                };
                let end = if line == end_position.line {
                    end_position.character
                } else {
                    line_length.saturating_sub(1)
                };
                (
                    start.min(line_length),
                    end.min(line_length.saturating_sub(1)),
                )
            }
        };
        Some(range)
    }

    pub fn set_system_clipboard(&mut self, text: String) {
        self.registers.set_clipboard_text(text);
    }

    pub fn take_system_clipboard_write(&mut self) -> Option<String> {
        self.registers.take_clipboard_write()
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
            Mode::Normal | Mode::OperatorPending => {
                if let Some(command) = self.vim.normal_command(key) {
                    self.execute_normal(command);
                    self.message.clear();
                }
            }
            Mode::VisualCharacter | Mode::VisualLine | Mode::VisualBlock => {
                if let Some(command) = self.vim.visual_command(key) {
                    self.execute_normal(command);
                    self.message.clear();
                }
            }
            Mode::Insert => self.handle_insert_key(key),
            Mode::Replace => self.handle_replace_key(key),
        }
    }

    fn execute_normal(&mut self, command: Command) {
        match command {
            Command::Move { motion, count } => self.execute_motion(motion, count),
            Command::EnterInsert(placement) => self.enter_insert(placement),
            Command::Operate(command) => self.execute_operator(command),
            Command::EnterVisual(kind) => self.enter_visual(kind),
            Command::ExitVisual => self.exit_visual(),
            Command::SwapVisualEnd => self.swap_visual_end(),
            Command::OperateSelection { operator, register } => {
                self.operate_selection(operator, register)
            }
            Command::DeleteCharacters { count, backward } => {
                self.delete_characters(count, backward)
            }
            Command::EnterReplace => self.enter_replace(),
            Command::ReplaceCharacter { character, count } => {
                self.replace_characters(character, count)
            }
            Command::BeginBlockInsert { append } => self.begin_block_insert(append),
            Command::Paste {
                after,
                count,
                register,
            } => self.paste(after, count, register),
        }
    }

    fn execute_motion(&mut self, motion: Motion, count: u32) {
        let desired = self
            .cursor
            .desired_character
            .unwrap_or(self.cursor.position.character);
        let Some(outcome) = resolve_motion(
            &self.buffer,
            self.cursor.position,
            desired,
            motion,
            count,
            self.last_find,
            self.visible_rows,
        ) else {
            return;
        };
        if let Some(find) = outcome.find {
            self.last_find = Some(find);
        }
        self.cursor.position = outcome.destination;
        self.cursor.desired_character = if matches!(
            motion,
            Motion::Down | Motion::Up | Motion::PageDown | Motion::PageUp | Motion::GoToLine { .. }
        ) {
            Some(desired)
        } else {
            None
        };
        self.ensure_cursor_visible();
    }

    fn enter_visual(&mut self, kind: VisualKind) {
        if self.vim.visual_kind() == Some(kind) {
            self.exit_visual();
            return;
        }
        if self.visual_anchor.is_none() {
            self.visual_anchor = Some(self.cursor.position);
        }
        self.vim.enter_visual(kind);
    }

    fn exit_visual(&mut self) {
        self.visual_anchor = None;
        self.vim.leave_transient_mode();
        self.clamp_cursor_to_mode();
    }

    fn swap_visual_end(&mut self) {
        if let Some(anchor) = &mut self.visual_anchor {
            std::mem::swap(anchor, &mut self.cursor.position);
        }
        self.ensure_cursor_visible();
    }

    fn visual_targets(&self) -> Vec<ResolvedTarget> {
        let Some(anchor) = self.visual_anchor else {
            return Vec::new();
        };
        let Some(kind) = self.vim.visual_kind() else {
            return Vec::new();
        };
        let head = self.cursor.position;
        match kind {
            VisualKind::Character => {
                let (start_position, end_position) = ordered_positions(anchor, head);
                let Ok(start) = self.buffer.position_to_offset(start_position) else {
                    return Vec::new();
                };
                let Ok(end_offset) = self.buffer.position_to_offset(end_position) else {
                    return Vec::new();
                };
                let end = CharOffset((end_offset.0 + 1).min(self.buffer.len_chars()));
                vec![ResolvedTarget {
                    start,
                    end,
                    shape: SelectionShape::CharacterWise,
                    cursor: start_position,
                }]
            }
            VisualKind::Line => {
                let first = anchor.line.min(head.line);
                let last = anchor.line.max(head.line);
                let Some(start) = self.buffer.line_start_offset(first) else {
                    return Vec::new();
                };
                let end = self
                    .buffer
                    .line_start_offset(last + 1)
                    .unwrap_or(CharOffset(self.buffer.len_chars()));
                vec![ResolvedTarget {
                    start,
                    end,
                    shape: SelectionShape::LineWise,
                    cursor: BufferPosition {
                        line: first,
                        character: 0,
                    },
                }]
            }
            VisualKind::Block => {
                let first_line = anchor.line.min(head.line);
                let last_line = anchor.line.max(head.line);
                let first_column = anchor.character.min(head.character);
                let last_column = anchor.character.max(head.character);
                (first_line..=last_line)
                    .filter_map(|line| {
                        let line_length = self.buffer.line_len_chars(line)?;
                        if first_column >= line_length {
                            return None;
                        }
                        let line_start = self.buffer.line_start_offset(line)?;
                        let start = CharOffset(line_start.0 + first_column);
                        let end = CharOffset(line_start.0 + (last_column + 1).min(line_length));
                        Some(ResolvedTarget {
                            start,
                            end,
                            shape: SelectionShape::BlockWise,
                            cursor: BufferPosition {
                                line,
                                character: first_column,
                            },
                        })
                    })
                    .collect()
            }
        }
    }

    fn operate_selection(&mut self, operator: Operator, register: RegisterName) {
        let kind = self.vim.visual_kind();
        let targets = self.visual_targets();
        if targets.is_empty() {
            self.exit_visual();
            return;
        }
        if matches!(
            operator,
            Operator::Delete | Operator::Change | Operator::Yank
        ) {
            let text = targets
                .iter()
                .filter_map(|target| self.buffer.text(target.start, target.end))
                .collect::<Vec<_>>()
                .join(if kind == Some(VisualKind::Block) {
                    "\n"
                } else {
                    ""
                });
            let value = RegisterValue {
                text,
                shape: match kind {
                    Some(VisualKind::Line) => SelectionShape::LineWise,
                    Some(VisualKind::Block) => SelectionShape::BlockWise,
                    _ => SelectionShape::CharacterWise,
                },
            };
            if operator == Operator::Yank {
                self.registers.record_yank(register, value);
            } else {
                self.registers.record_delete(register, value);
            }
        }

        match operator {
            Operator::Yank => {
                self.cursor.position = targets[0].cursor;
                self.exit_visual();
            }
            Operator::Delete => {
                self.exit_visual();
                self.delete_multiple_targets(&targets);
            }
            Operator::Change if kind == Some(VisualKind::Block) => {
                let lines: Vec<_> = targets.iter().map(|target| target.cursor.line).collect();
                let column = targets[0].cursor.character;
                self.exit_visual();
                self.buffer.begin_transaction();
                for target in targets.iter().rev() {
                    let _ = self.buffer.remove(target.start, target.end);
                }
                self.cursor.position = BufferPosition {
                    line: lines[0],
                    character: column,
                };
                self.block_insert = Some(BlockInsertState {
                    lines,
                    primary_line: self.cursor.position.line,
                    column,
                    inserted_text: String::new(),
                });
                self.insert_session_changed = true;
                self.vim.enter_insert();
            }
            Operator::Change => {
                let target = targets[0];
                self.exit_visual();
                self.change_target(target);
            }
            Operator::Indent | Operator::Outdent | Operator::Reindent => {
                let mut target = targets[0];
                if let Some(last) = targets.last() {
                    target.end = last.end;
                }
                self.exit_visual();
                self.indent_target(target, operator);
            }
            Operator::SwapCase | Operator::Lowercase | Operator::Uppercase => {
                self.exit_visual();
                for target in targets.iter().rev() {
                    self.case_target(*target, operator);
                }
            }
        }
    }

    fn delete_multiple_targets(&mut self, targets: &[ResolvedTarget]) {
        self.buffer.begin_transaction();
        for target in targets.iter().rev() {
            let _ = self.buffer.remove(target.start, target.end);
        }
        self.buffer.commit_transaction();
        let first = targets[0];
        self.cursor.position = self
            .buffer
            .offset_to_position(CharOffset(first.start.0.min(self.buffer.len_chars())))
            .unwrap_or_default();
        self.clamp_cursor_to_mode();
    }

    fn begin_block_insert(&mut self, append: bool) {
        let Some(anchor) = self.visual_anchor else {
            return;
        };
        let head = self.cursor.position;
        let first_line = anchor.line.min(head.line);
        let last_line = anchor.line.max(head.line);
        let column = if append {
            anchor.character.max(head.character).saturating_add(1)
        } else {
            anchor.character.min(head.character)
        };
        let lines: Vec<_> = (first_line..=last_line).collect();
        self.exit_visual();
        self.buffer.begin_transaction();
        self.ensure_line_column(first_line, column);
        self.cursor.position = BufferPosition {
            line: first_line,
            character: column,
        };
        self.block_insert = Some(BlockInsertState {
            lines,
            primary_line: first_line,
            column,
            inserted_text: String::new(),
        });
        self.insert_session_changed = false;
        self.vim.enter_insert();
    }

    fn delete_characters(&mut self, count: u32, backward: bool) {
        let line = self.cursor.position.line;
        let line_length = self.buffer.line_len_chars(line).unwrap_or(0);
        if line_length == 0 {
            return;
        }
        let count = count.max(1) as usize;
        let (start_character, end_character) = if backward {
            let end = self.cursor.position.character;
            (end.saturating_sub(count), end)
        } else {
            let start = self.cursor.position.character;
            (start, start.saturating_add(count).min(line_length))
        };
        if start_character == end_character {
            return;
        }
        let start = self
            .buffer
            .position_to_offset(BufferPosition {
                line,
                character: start_character,
            })
            .expect("valid character deletion start");
        let end = self
            .buffer
            .position_to_offset(BufferPosition {
                line,
                character: end_character,
            })
            .expect("valid character deletion end");
        let target = ResolvedTarget {
            start,
            end,
            shape: SelectionShape::CharacterWise,
            cursor: BufferPosition {
                line,
                character: start_character,
            },
        };
        self.registers.record_delete(
            RegisterName::Unnamed,
            RegisterValue {
                text: self.buffer.text(start, end).unwrap_or_default(),
                shape: SelectionShape::CharacterWise,
            },
        );
        self.delete_target(target);
    }

    fn paste(&mut self, after: bool, count: u32, register: RegisterName) {
        let Some(value) = self.registers.read(register).cloned() else {
            return;
        };
        let repeated = value.text.repeat(count.max(1) as usize);
        self.buffer.begin_transaction();
        match value.shape {
            SelectionShape::CharacterWise => {
                let line_length = self.current_line_length();
                let character = if after {
                    self.cursor
                        .position
                        .character
                        .saturating_add(1)
                        .min(line_length)
                } else {
                    self.cursor.position.character
                };
                if let Ok(offset) = self.buffer.position_to_offset(BufferPosition {
                    line: self.cursor.position.line,
                    character,
                }) {
                    let _ = self.buffer.insert(offset, &repeated);
                    self.cursor.position = self
                        .buffer
                        .offset_to_position(CharOffset(
                            offset.0 + repeated.chars().count().saturating_sub(1),
                        ))
                        .unwrap_or(self.cursor.position);
                }
            }
            SelectionShape::LineWise => {
                let line = if after {
                    self.cursor.position.line.saturating_add(1)
                } else {
                    self.cursor.position.line
                };
                let offset = self
                    .buffer
                    .line_start_offset(line)
                    .unwrap_or(CharOffset(self.buffer.len_chars()));
                let mut text = repeated;
                if after
                    && line >= self.buffer.len_lines()
                    && self.buffer.len_chars() > 0
                    && self.buffer.char_at(CharOffset(self.buffer.len_chars() - 1)) != Some('\n')
                {
                    text.insert(0, '\n');
                }
                if !text.ends_with('\n') && line < self.buffer.len_lines() {
                    text.push('\n');
                }
                let _ = self.buffer.insert(offset, &text);
                self.cursor.position = self.buffer.offset_to_position(offset).unwrap_or_default();
            }
            SelectionShape::BlockWise => {
                let column = self.cursor.position.character + usize::from(after);
                let rows: Vec<_> = value.text.lines().collect();
                for _ in 0..count.max(1) {
                    for (row, text) in rows.iter().enumerate().rev() {
                        let line = self.cursor.position.line + row;
                        if line >= self.buffer.len_lines() {
                            continue;
                        }
                        self.ensure_line_column(line, column);
                        if let Some(start) = self.buffer.line_start_offset(line) {
                            let _ = self.buffer.insert(CharOffset(start.0 + column), text);
                        }
                    }
                }
            }
        }
        self.buffer.commit_transaction();
        self.clamp_cursor_to_mode();
    }

    fn enter_replace(&mut self) {
        self.buffer.begin_transaction();
        self.insert_session_changed = false;
        self.vim.enter_replace();
    }

    fn replace_characters(&mut self, character: char, count: u32) {
        let line = self.cursor.position.line;
        let line_length = self.buffer.line_len_chars(line).unwrap_or(0);
        let count = count.max(1) as usize;
        let start_character = self.cursor.position.character;
        if start_character.saturating_add(count) > line_length {
            return;
        }
        let start = self.buffer.position_to_offset(self.cursor.position).ok();
        let end = self
            .buffer
            .position_to_offset(BufferPosition {
                line,
                character: start_character + count,
            })
            .ok();
        let (Some(start), Some(end)) = (start, end) else {
            return;
        };
        self.buffer.begin_transaction();
        let _ = self.buffer.remove(start, end);
        let replacement: String = std::iter::repeat_n(character, count).collect();
        let _ = self.buffer.insert(start, &replacement);
        self.buffer.commit_transaction();
        self.cursor.position.character = start_character + count.saturating_sub(1);
    }

    fn execute_operator(&mut self, mut command: OperatorCommand) {
        if command.operator == Operator::Change {
            command.target = match command.target {
                Target::Motion(Motion::WordForward { big_word }) => {
                    Target::Motion(Motion::WordEnd { big_word })
                }
                target => target,
            };
        }
        let desired = self
            .cursor
            .desired_character
            .unwrap_or(self.cursor.position.character);
        let Some(target) = resolve_target(
            &self.buffer,
            self.cursor.position,
            desired,
            command.target,
            command.count,
            self.last_find,
            self.visible_rows,
        ) else {
            return;
        };
        if let Target::Motion(Motion::FindCharacter(find)) = command.target {
            self.last_find = Some(find);
        }

        match command.operator {
            Operator::Delete | Operator::Change | Operator::Yank => {
                let value = RegisterValue {
                    text: self
                        .buffer
                        .text(target.start, target.end)
                        .unwrap_or_default(),
                    shape: target.shape,
                };
                match command.operator {
                    Operator::Yank => self.registers.record_yank(command.register, value),
                    _ => self.registers.record_delete(command.register, value),
                }
                match command.operator {
                    Operator::Delete => self.delete_target(target),
                    Operator::Change => self.change_target(target),
                    Operator::Yank => {}
                    _ => unreachable!(),
                }
            }
            Operator::Indent | Operator::Outdent | Operator::Reindent => {
                self.indent_target(target, command.operator)
            }
            Operator::SwapCase | Operator::Lowercase | Operator::Uppercase => {
                self.case_target(target, command.operator)
            }
        }
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn delete_target(&mut self, target: crate::vim::ResolvedTarget) {
        self.buffer.begin_transaction();
        if self.buffer.remove(target.start, target.end).is_ok() {
            let offset = CharOffset(target.start.0.min(self.buffer.len_chars()));
            self.cursor.position = self.buffer.offset_to_position(offset).unwrap_or_default();
            self.buffer.commit_transaction();
            self.clamp_cursor_to_mode();
        } else {
            self.buffer.commit_transaction();
        }
    }

    fn change_target(&mut self, target: crate::vim::ResolvedTarget) {
        let had_following_text = target.end.0 < self.buffer.len_chars();
        self.buffer.begin_transaction();
        if self.buffer.remove(target.start, target.end).is_err() {
            self.buffer.commit_transaction();
            return;
        }
        if target.shape == SelectionShape::LineWise && had_following_text {
            let _ = self.buffer.insert(target.start, "\n");
        }
        let offset = CharOffset(target.start.0.min(self.buffer.len_chars()));
        self.cursor.position = self.buffer.offset_to_position(offset).unwrap_or_default();
        if target.shape == SelectionShape::LineWise {
            self.cursor.position.character = 0;
        }
        self.insert_session_changed = true;
        self.vim.enter_insert();
    }

    fn case_target(&mut self, target: crate::vim::ResolvedTarget, operator: Operator) {
        let Some(text) = self.buffer.text(target.start, target.end) else {
            return;
        };
        let replacement: String = text
            .chars()
            .flat_map(|character| match operator {
                Operator::SwapCase if character.is_lowercase() => {
                    character.to_uppercase().collect::<Vec<_>>()
                }
                Operator::SwapCase if character.is_uppercase() => {
                    character.to_lowercase().collect::<Vec<_>>()
                }
                Operator::Lowercase => character.to_lowercase().collect(),
                Operator::Uppercase => character.to_uppercase().collect(),
                _ => vec![character],
            })
            .collect();
        self.buffer.begin_transaction();
        let _ = self.buffer.remove(target.start, target.end);
        let _ = self.buffer.insert(target.start, &replacement);
        self.buffer.commit_transaction();
        self.cursor.position = self
            .buffer
            .offset_to_position(CharOffset(target.start.0.min(self.buffer.len_chars())))
            .unwrap_or_default();
        self.clamp_cursor_to_mode();
    }

    fn indent_target(&mut self, target: crate::vim::ResolvedTarget, operator: Operator) {
        let (first_line, last_line) = self.target_lines(target);
        let mut edits = Vec::new();
        let mut depth = self.brace_depth_before(first_line);
        for line in first_line..=last_line {
            let text = self.buffer.line_text(line).unwrap_or_default();
            let leading = text
                .chars()
                .take_while(|character| character.is_whitespace())
                .count();
            let (remove, insert) = match operator {
                Operator::Indent => (0, "    ".to_owned()),
                Operator::Outdent => {
                    let remove = if text.starts_with('\t') {
                        1
                    } else {
                        leading.min(4)
                    };
                    (remove, String::new())
                }
                Operator::Reindent => {
                    let trimmed = text.trim_start();
                    let line_depth = if trimmed.starts_with('}') {
                        depth.saturating_sub(1)
                    } else {
                        depth
                    };
                    (leading, " ".repeat(line_depth.saturating_mul(4)))
                }
                _ => unreachable!(),
            };
            edits.push((line, remove, insert));
            if operator == Operator::Reindent {
                depth = updated_brace_depth(depth, &text);
            }
        }

        self.buffer.begin_transaction();
        for (line, remove, insert) in edits.into_iter().rev() {
            let Some(start) = self.buffer.line_start_offset(line) else {
                continue;
            };
            if remove > 0 {
                let _ = self
                    .buffer
                    .remove(start, CharOffset(start.0.saturating_add(remove)));
            }
            if !insert.is_empty() {
                let _ = self.buffer.insert(start, &insert);
            }
        }
        self.buffer.commit_transaction();
        self.cursor.position = BufferPosition {
            line: first_line,
            character: 0,
        };
    }

    fn target_lines(&self, target: crate::vim::ResolvedTarget) -> (usize, usize) {
        let first = self
            .buffer
            .offset_to_position(target.start)
            .unwrap_or_default()
            .line;
        let last_offset = CharOffset(target.end.0.saturating_sub(1));
        let last = self
            .buffer
            .offset_to_position(last_offset)
            .unwrap_or_default()
            .line;
        (first, last)
    }

    fn brace_depth_before(&self, line: usize) -> usize {
        (0..line).fold(0, |depth, index| {
            updated_brace_depth(depth, &self.buffer.line_text(index).unwrap_or_default())
        })
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

    fn handle_replace_key(&mut self, key: &Key) {
        match &key.value {
            KeyValue::Named(name) if name == "escape" => self.leave_insert(),
            KeyValue::Named(name) if name == "backspace" => self.move_horizontal(-1),
            KeyValue::Named(name) if name == "enter" => self.insert_text("\n"),
            KeyValue::Named(name) if name == "left" => self.move_horizontal(-1),
            KeyValue::Named(name) if name == "right" => self.move_horizontal(1),
            KeyValue::Named(name) if name == "up" => self.move_vertical(-1),
            KeyValue::Named(name) if name == "down" => self.move_vertical(1),
            KeyValue::Character(text)
                if !key.modifiers.control && !key.modifiers.alt && !key.modifiers.super_key =>
            {
                self.replace_text(text)
            }
            _ => return,
        }
        self.message.clear();
    }

    fn leave_insert(&mut self) {
        self.finish_block_insert();
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
        if !text.contains('\n')
            && let Some(block) = &mut self.block_insert
        {
            block.inserted_text.push_str(text);
        }
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

    fn replace_text(&mut self, text: &str) {
        let Ok(offset) = self.buffer.position_to_offset(self.cursor.position) else {
            return;
        };
        let available = self
            .current_line_length()
            .saturating_sub(self.cursor.position.character);
        let remove_count = text.chars().count().min(available);
        if remove_count > 0 {
            let _ = self
                .buffer
                .remove(offset, CharOffset(offset.0.saturating_add(remove_count)));
        }
        if self.buffer.insert(offset, text).is_err() {
            return;
        }
        let new_offset = CharOffset(offset.0 + text.chars().count());
        self.cursor.position = self
            .buffer
            .offset_to_position(new_offset)
            .unwrap_or_default();
        self.insert_session_changed = true;
        self.ensure_cursor_visible();
    }

    fn finish_block_insert(&mut self) {
        let Some(block) = self.block_insert.take() else {
            return;
        };
        if block.inserted_text.is_empty() {
            return;
        }
        for line in block.lines.into_iter().rev() {
            if line == block.primary_line {
                continue;
            }
            self.ensure_line_column(line, block.column);
            if let Some(start) = self.buffer.line_start_offset(line) {
                let _ = self
                    .buffer
                    .insert(CharOffset(start.0 + block.column), &block.inserted_text);
            }
        }
    }

    fn ensure_line_column(&mut self, line: usize, column: usize) {
        let line_length = self.buffer.line_len_chars(line).unwrap_or(0);
        if line_length >= column {
            return;
        }
        if let Some(start) = self.buffer.line_start_offset(line) {
            let _ = self.buffer.insert(
                CharOffset(start.0 + line_length),
                &" ".repeat(column - line_length),
            );
        }
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

    fn current_line_length(&self) -> usize {
        self.buffer
            .line_len_chars(self.cursor.position.line)
            .unwrap_or(0)
    }

    fn maximum_character(&self, line: usize) -> usize {
        let line_length = self.buffer.line_len_chars(line).unwrap_or(0);
        if matches!(self.vim.mode(), Mode::Insert | Mode::Replace) {
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

fn ordered_positions(
    first: BufferPosition,
    second: BufferPosition,
) -> (BufferPosition, BufferPosition) {
    if (first.line, first.character) <= (second.line, second.character) {
        (first, second)
    } else {
        (second, first)
    }
}

fn updated_brace_depth(mut depth: usize, text: &str) -> usize {
    for character in text.chars() {
        match character {
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
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

    fn control_named(name: &str) -> Key {
        Key {
            value: KeyValue::Named(name.to_owned()),
            modifiers: Modifiers {
                control: true,
                ..Modifiers::default()
            },
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

    #[test]
    fn word_motions_distinguish_words_and_punctuation() {
        let mut editor = editor_with_text("one two.three");

        type_keys(&mut editor, "w");
        assert_eq!(editor.cursor.position.character, 4);
        type_keys(&mut editor, "w");
        assert_eq!(editor.cursor.position.character, 7);
        type_keys(&mut editor, "e");
        assert_eq!(editor.cursor.position.character, 12);
        type_keys(&mut editor, "b");
        assert_eq!(editor.cursor.position.character, 8);
    }

    #[test]
    fn delete_composes_with_word_and_line_targets() {
        let mut words = editor_with_text("one two three");
        type_keys(&mut words, "2dw");
        assert_eq!(words.buffer.line_text(0).as_deref(), Some("three"));
        let register = words.registers.unnamed().expect("unnamed register");
        assert_eq!(register.text, "one two ");
        assert_eq!(register.shape, SelectionShape::CharacterWise);

        let mut lines = editor_with_text("one\ntwo\nthree");
        type_keys(&mut lines, "2dd");
        assert_eq!(lines.buffer.line_text(0).as_deref(), Some("three"));
        assert_eq!(
            lines.registers.unnamed().expect("unnamed register").shape,
            SelectionShape::LineWise
        );
    }

    #[test]
    fn change_word_keeps_one_transaction_open_through_insert() {
        let mut editor = editor_with_text("one two");

        type_keys(&mut editor, "cwnew");
        editor.handle_key(&named("escape"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("new two"));
        assert_eq!(editor.buffer.completed_transaction_count(), 1);
    }

    #[test]
    fn yank_and_black_hole_register_follow_operator_targets() {
        let mut editor = editor_with_text("one two three");
        type_keys(&mut editor, "yw");
        assert_eq!(
            editor.registers.unnamed().expect("unnamed register").text,
            "one "
        );

        type_keys(&mut editor, "w\"_dw");
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one three"));
        assert_eq!(
            editor.registers.unnamed().expect("unnamed register").text,
            "one "
        );
    }

    #[test]
    fn case_and_indent_operators_use_generic_targets() {
        let mut case = editor_with_text("one two");
        type_keys(&mut case, "gUw");
        assert_eq!(case.buffer.line_text(0).as_deref(), Some("ONE two"));

        let mut indent = editor_with_text("one\ntwo");
        type_keys(&mut indent, ">>");
        assert_eq!(indent.buffer.line_text(0).as_deref(), Some("    one"));
        type_keys(&mut indent, "<<");
        assert_eq!(indent.buffer.line_text(0).as_deref(), Some("one"));

        let mut reindent = editor_with_text("{\nfoo\n}");
        type_keys(&mut reindent, "j==");
        assert_eq!(reindent.buffer.line_text(1).as_deref(), Some("    foo"));
    }

    #[test]
    fn delete_right_at_line_end_still_deletes_the_cursor_character() {
        let mut editor = editor_with_text("abc");
        type_keys(&mut editor, "$dl");
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("ab"));
    }

    #[test]
    fn big_word_and_backward_find_motions_work() {
        let mut words = editor_with_text("one.two three");
        type_keys(&mut words, "W");
        assert_eq!(words.cursor.position.character, 8);
        type_keys(&mut words, "E");
        assert_eq!(words.cursor.position.character, 12);
        type_keys(&mut words, "B");
        assert_eq!(words.cursor.position.character, 8);

        let mut find = editor_with_text("a x b x");
        type_keys(&mut find, "$Fx");
        assert_eq!(find.cursor.position.character, 2);
        type_keys(&mut find, "$Tx");
        assert_eq!(find.cursor.position.character, 3);
    }

    #[test]
    fn find_repeat_and_matching_delimiter_motions_work() {
        let mut find = editor_with_text("a x b x");
        type_keys(&mut find, "fx");
        assert_eq!(find.cursor.position.character, 2);
        type_keys(&mut find, ";");
        assert_eq!(find.cursor.position.character, 6);
        type_keys(&mut find, ",");
        assert_eq!(find.cursor.position.character, 2);

        let mut delimiter = editor_with_text("(a(b)c)");
        type_keys(&mut delimiter, "%");
        assert_eq!(delimiter.cursor.position.character, 6);
        type_keys(&mut delimiter, "0d%");
        assert_eq!(delimiter.buffer.line_text(0).as_deref(), Some(""));
    }

    #[test]
    fn escape_cancels_operator_pending_mode() {
        let mut editor = editor_with_text("one");
        type_keys(&mut editor, "d");
        assert_eq!(editor.mode(), Mode::OperatorPending);
        editor.handle_key(&named("escape"));
        assert_eq!(editor.mode(), Mode::Normal);
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one"));
    }

    #[test]
    fn x_and_x_delete_characters_with_counts() {
        let mut forward = editor_with_text("abcdef");
        type_keys(&mut forward, "2x");
        assert_eq!(forward.buffer.line_text(0).as_deref(), Some("cdef"));

        let mut backward = editor_with_text("abcdef");
        type_keys(&mut backward, "lll2X");
        assert_eq!(backward.buffer.line_text(0).as_deref(), Some("adef"));
    }

    #[test]
    fn visual_character_and_line_selections_use_existing_operators() {
        let mut characters = editor_with_text("one two");
        type_keys(&mut characters, "vlld");
        assert_eq!(characters.buffer.line_text(0).as_deref(), Some(" two"));
        assert_eq!(characters.mode(), Mode::Normal);

        let mut lines = editor_with_text("one\ntwo\nthree");
        type_keys(&mut lines, "Vjd");
        assert_eq!(lines.buffer.line_text(0).as_deref(), Some("three"));
    }

    #[test]
    fn visual_block_delete_applies_disjoint_ranges() {
        let mut editor = editor_with_text("abc\ndef\nghi");
        editor.handle_key(&control_named("v"));
        type_keys(&mut editor, "ljd");

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("c"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("f"));
        assert_eq!(editor.buffer.line_text(2).as_deref(), Some("ghi"));
        assert_eq!(
            editor.registers.unnamed().expect("block register").shape,
            SelectionShape::BlockWise
        );
    }

    #[test]
    fn visual_block_insert_replicates_text_in_one_transaction() {
        let mut editor = editor_with_text("abc\ndef\nghi");
        editor.handle_key(&control_named("v"));
        type_keys(&mut editor, "jjI>");
        editor.handle_key(&named("escape"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some(">abc"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some(">def"));
        assert_eq!(editor.buffer.line_text(2).as_deref(), Some(">ghi"));
        assert_eq!(editor.buffer.completed_transaction_count(), 1);
    }

    #[test]
    fn single_and_continuous_replace_modes_modify_in_place() {
        let mut single = editor_with_text("abcd");
        type_keys(&mut single, "2rX");
        assert_eq!(single.buffer.line_text(0).as_deref(), Some("XXcd"));
        assert_eq!(single.cursor.position.character, 1);

        let mut continuous = editor_with_text("abcd");
        type_keys(&mut continuous, "RXY");
        continuous.handle_key(&named("escape"));
        assert_eq!(continuous.buffer.line_text(0).as_deref(), Some("XYcd"));
        assert_eq!(continuous.mode(), Mode::Normal);
        assert_eq!(continuous.buffer.completed_transaction_count(), 1);
    }

    #[test]
    fn visual_change_enters_insert_and_commits_once() {
        let mut editor = editor_with_text("one two");
        type_keys(&mut editor, "vllcnew");
        editor.handle_key(&named("escape"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("new two"));
        assert_eq!(editor.buffer.completed_transaction_count(), 1);
    }

    #[test]
    fn word_quote_delimiter_and_tag_text_objects_resolve_generically() {
        let mut word = editor_with_text("one two");
        type_keys(&mut word, "diw");
        assert_eq!(word.buffer.line_text(0).as_deref(), Some(" two"));

        let mut around = editor_with_text("one two");
        type_keys(&mut around, "daw");
        assert_eq!(around.buffer.line_text(0).as_deref(), Some("two"));

        let mut quote = editor_with_text("say \"hello\" now");
        type_keys(&mut quote, "llllldi\"");
        assert_eq!(quote.buffer.line_text(0).as_deref(), Some("say \"\" now"));

        let mut delimiter = editor_with_text("a (nested) value");
        type_keys(&mut delimiter, "llllda(");
        assert_eq!(delimiter.buffer.line_text(0).as_deref(), Some("a  value"));

        let mut tag = editor_with_text("<b>bold</b>");
        type_keys(&mut tag, "lllldit");
        assert_eq!(tag.buffer.line_text(0).as_deref(), Some("<b></b>"));
    }

    #[test]
    fn character_and_linewise_paste_preserve_register_shape() {
        let mut character = editor_with_text("one two");
        type_keys(&mut character, "yiw$p");
        assert_eq!(character.buffer.line_text(0).as_deref(), Some("one twoone"));

        let mut line = editor_with_text("one\ntwo");
        type_keys(&mut line, "yyp");
        assert_eq!(line.buffer.line_text(0).as_deref(), Some("one"));
        assert_eq!(line.buffer.line_text(1).as_deref(), Some("one"));
        assert_eq!(line.buffer.line_text(2).as_deref(), Some("two"));
    }

    #[test]
    fn blockwise_paste_applies_each_register_row() {
        let mut editor = editor_with_text("ab\ncd");
        editor.handle_key(&control_named("v"));
        type_keys(&mut editor, "jyP");

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("aab"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("ccd"));
    }

    #[test]
    fn named_append_and_clipboard_registers_work() {
        let mut editor = editor_with_text("one two");
        type_keys(&mut editor, "\"ayiw");
        type_keys(&mut editor, "w\"Ayiw");
        assert_eq!(
            editor
                .registers
                .read(RegisterName::Named {
                    name: 'a',
                    append: false
                })
                .expect("named register")
                .text,
            "onetwo"
        );

        editor.set_system_clipboard("clip".to_owned());
        type_keys(&mut editor, "$\"+p");
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one twoclip"));

        let mut write = editor_with_text("copy");
        type_keys(&mut write, "\"+yiw");
        assert_eq!(write.take_system_clipboard_write().as_deref(), Some("copy"));
    }

    #[test]
    fn remaining_line_motions_land_on_first_non_blank() {
        let mut editor = editor_with_text("  one\n    two\n three");
        type_keys(&mut editor, "+");
        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 1,
                character: 4
            }
        );
        type_keys(&mut editor, "-");
        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 0,
                character: 2
            }
        );
        type_keys(&mut editor, "2_");
        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 1,
                character: 4
            }
        );
    }

    fn editor_with_text(text: &str) -> Editor {
        let mut editor = Editor::scratch();
        editor
            .buffer
            .insert(CharOffset(0), text)
            .expect("valid insertion");
        editor
    }
}
