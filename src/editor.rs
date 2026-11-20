use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use regex::{Regex, RegexBuilder};

use crate::{
    buffer::{Buffer, BufferPosition, CharOffset},
    dired::{self, DiredCommand, DiredState},
    ex::{self, Address, ExCommand, GlobalAction, RangeSeparator, RangeSpec, Substitute},
    input::{Key, KeyValue},
    minibuffer::{Minibuffer, MinibufferKind, ShellPathCompletion},
    navigation::{ChangeList, JumpList},
    output::{JumpTarget, OutputState},
    search::{self, Search},
    shell::{ShellCompletion, ShellRequest},
    vim::{
        Command, Direction, FindMotion, InsertPlacement, Mode, Motion, Operator, OperatorCommand,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditorRequest {
    Close,
    RunShell {
        request: ShellRequest,
        open_immediately: bool,
    },
}

#[derive(Clone, Debug)]
struct DiredReturn {
    directory: PathBuf,
    selected_path: PathBuf,
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
    dired: Option<DiredState>,
    dired_return: Option<DiredReturn>,
    output: Option<OutputState>,
    output_return: Option<PathBuf>,
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
    macros: HashMap<char, Vec<Key>>,
    recording_macro: Option<(char, Vec<Key>)>,
    last_macro: Option<char>,
    pending_change_keys: Vec<Key>,
    last_change_keys: Vec<Key>,
    change_list: ChangeList,
    jump_list: JumpList,
    marks: HashMap<char, BufferPosition>,
    minibuffer: Option<Minibuffer>,
    last_search: Option<Search>,
    last_ex_pattern: Option<String>,
    request: Option<EditorRequest>,
    next_shell_request_id: u64,
    replaying: bool,
}

impl Editor {
    #[cfg(test)]
    pub fn scratch() -> Self {
        Self::with_buffer(Buffer::scratch(), None, String::new())
    }

    pub fn dired_current() -> Self {
        match std::env::current_dir() {
            Ok(directory) => Self::dired(directory),
            Err(error) => Self::with_buffer(
                Buffer::scratch(),
                None,
                format!("Could not determine current directory: {error}"),
            ),
        }
    }

    pub fn dired(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        match DiredState::load(path) {
            Ok((buffer, dired)) => Self::with_buffer(buffer, Some(dired), String::new()),
            Err(error) => Self::with_buffer(
                Buffer::scratch(),
                None,
                format!("Could not open directory {}: {error}", path.display()),
            ),
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        if path.is_dir() {
            return Self::dired(path);
        }
        match Buffer::open(path) {
            Ok(buffer) => Self::with_buffer(buffer, None, String::new()),
            Err(error) => Self::with_buffer(
                Buffer::scratch(),
                None,
                format!("Could not open {}: {error}", path.display()),
            ),
        }
    }

    fn with_buffer(buffer: Buffer, dired: Option<DiredState>, message: String) -> Self {
        Self {
            buffer,
            dired,
            dired_return: None,
            output: None,
            output_return: None,
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
            macros: HashMap::new(),
            recording_macro: None,
            last_macro: None,
            pending_change_keys: Vec::new(),
            last_change_keys: Vec::new(),
            change_list: ChangeList::default(),
            jump_list: JumpList::default(),
            marks: HashMap::new(),
            minibuffer: None,
            last_search: None,
            last_ex_pattern: None,
            request: None,
            next_shell_request_id: 1,
            replaying: false,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn display_name(&self) -> String {
        if let Some(dired) = &self.dired {
            format!("Dired {}", dired.directory().display())
        } else if let Some(output) = &self.output {
            output.display_name()
        } else {
            self.buffer.display_name()
        }
    }

    pub fn is_dired(&self) -> bool {
        self.dired.is_some()
    }

    pub fn is_output(&self) -> bool {
        self.output.is_some()
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

    pub fn prompt_text(&self) -> Option<String> {
        self.minibuffer.as_ref().map(Minibuffer::display)
    }

    pub fn take_request(&mut self) -> Option<EditorRequest> {
        self.request.take()
    }

    pub fn append_shell_output(&mut self, request_id: u64, text: &str) {
        if text.is_empty()
            || !self
                .output
                .as_ref()
                .is_some_and(|output| output.is_running_request(request_id))
        {
            return;
        }
        if self
            .buffer
            .insert(CharOffset(self.buffer.len_chars()), text)
            .is_err()
        {
            return;
        }
        self.cursor.position.line = self.buffer.len_lines().saturating_sub(1);
        self.cursor.position.character = self.maximum_character(self.cursor.position.line);
        self.ensure_cursor_visible();
    }

    pub fn finish_shell(&mut self, completion: ShellCompletion, open_immediately: bool) {
        if open_immediately {
            let request_id = completion.request.id;
            if !self
                .output
                .as_ref()
                .is_some_and(|output| output.is_running_request(request_id))
            {
                return;
            }
        } else if !self.is_read_only_special() && self.buffer.is_modified() {
            self.message =
                "Shell finished, but output was not opened because the buffer changed".to_owned();
            return;
        }
        let (buffer, output) = OutputState::from_completion(completion);
        self.replace_buffer(buffer, None, Some(output));
        self.cursor.position.line = 1.min(self.buffer.len_lines().saturating_sub(1));
        self.message = "Shell command finished".to_owned();
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
        if !self.replaying
            && self.recording_macro.is_some()
            && self.minibuffer.is_none()
            && self.vim.mode() == Mode::Normal
            && matches!(&key.value, KeyValue::Character(value) if value == "q")
        {
            if let Some((register, keys)) = self.recording_macro.take() {
                self.macros.insert(register, keys);
            }
            return;
        }
        if !self.replaying
            && let Some((_, keys)) = &mut self.recording_macro
        {
            keys.push(key.clone());
        }
        if self.minibuffer.is_some() {
            let transactions_before = self.buffer.completed_transaction_count();
            self.handle_minibuffer_key(key);
            self.track_completed_transaction(transactions_before);
            return;
        }
        if self.output.is_some() && is_plain_named_key(key, "enter") {
            self.execute_output_target();
            return;
        }
        if self.dired.is_some()
            && let Some(command) = dired::command_for_key(key)
        {
            self.execute_dired(command);
            return;
        }
        if !self.replaying && (!self.pending_change_keys.is_empty() || starts_change(key)) {
            self.pending_change_keys.push(key.clone());
        }
        let transactions_before = self.buffer.completed_transaction_count();

        if matches!(&key.value, KeyValue::Named(name) if name == "s") && key.modifiers.control {
            self.save();
            return;
        }

        match self.vim.mode() {
            Mode::Normal | Mode::OperatorPending => {
                if let Some(command) = self.vim.normal_command(key) {
                    self.message.clear();
                    self.execute_normal(command);
                }
            }
            Mode::VisualCharacter | Mode::VisualLine | Mode::VisualBlock => {
                if let Some(command) = self.vim.visual_command(key) {
                    self.message.clear();
                    self.execute_normal(command);
                }
            }
            Mode::Insert => self.handle_insert_key(key),
            Mode::Replace => self.handle_replace_key(key),
        }

        self.track_completed_transaction(transactions_before);
        if !self.replaying
            && matches!(&key.value, KeyValue::Named(name) if name == "escape")
            && self.vim.mode() == Mode::Normal
        {
            self.pending_change_keys.clear();
        }
    }

    fn execute_normal(&mut self, command: Command) {
        if self.is_read_only_special() && is_mutating_command(command) {
            self.message = self.read_only_message().to_owned();
            self.visual_anchor = None;
            self.vim.leave_transient_mode();
            return;
        }
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
            Command::Undo { count } => self.undo(count),
            Command::Redo { count } => self.redo(count),
            Command::BeginMacro { register } => {
                self.pending_change_keys.clear();
                self.recording_macro = Some((register, Vec::new()));
            }
            Command::PlayMacro { register, count } => {
                self.pending_change_keys.clear();
                self.play_macro(register, count);
            }
            Command::RepeatChange { count } => self.repeat_change(count),
            Command::StartSearch { direction } => self.start_search(direction),
            Command::StartEx => self.start_ex(),
            Command::RepeatSearch { reverse, count } => self.repeat_search(reverse, count),
            Command::SearchWord { direction, count } => self.search_word(direction, count),
            Command::SetMark { mark } => {
                self.pending_change_keys.clear();
                self.marks.insert(mark, self.cursor.position);
            }
            Command::JumpToMark { mark, linewise } => {
                self.pending_change_keys.clear();
                self.jump_to_mark(mark, linewise);
            }
            Command::JumpOlder { count } => self.jump_older(count),
            Command::JumpNewer { count } => self.jump_newer(count),
            Command::ChangeOlder { count } => self.change_older(count),
            Command::ChangeNewer { count } => self.change_newer(count),
        }
    }

    fn execute_motion(&mut self, motion: Motion, count: u32) {
        let origin = self.cursor.position;
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
        if matches!(motion, Motion::GoToLine { .. } | Motion::MatchingDelimiter) {
            self.jump_list.record(origin, outcome.destination);
        }
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

    fn undo(&mut self, count: u32) {
        for _ in 0..count.max(1) {
            let Some(offset) = self.buffer.undo() else {
                break;
            };
            self.cursor.position = self
                .buffer
                .offset_to_position(CharOffset(offset.0.min(self.buffer.len_chars())))
                .unwrap_or_default();
        }
        self.vim.leave_transient_mode();
        self.visual_anchor = None;
        self.clamp_cursor_to_mode();
        self.ensure_cursor_visible();
    }

    fn redo(&mut self, count: u32) {
        for _ in 0..count.max(1) {
            let Some(offset) = self.buffer.redo() else {
                break;
            };
            self.cursor.position = self
                .buffer
                .offset_to_position(CharOffset(offset.0.min(self.buffer.len_chars())))
                .unwrap_or_default();
        }
        self.vim.leave_transient_mode();
        self.visual_anchor = None;
        self.clamp_cursor_to_mode();
        self.ensure_cursor_visible();
    }

    fn play_macro(&mut self, register: Option<char>, count: u32) {
        let Some(register) = register.or(self.last_macro) else {
            return;
        };
        let Some(keys) = self.macros.get(&register).cloned() else {
            return;
        };
        self.last_macro = Some(register);
        let was_replaying = self.replaying;
        self.replaying = true;
        for _ in 0..count.max(1) {
            for key in &keys {
                self.handle_key(key);
            }
        }
        self.replaying = was_replaying;
    }

    fn repeat_change(&mut self, count: u32) {
        if self.last_change_keys.is_empty() {
            return;
        }
        let keys = self.last_change_keys.clone();
        let was_replaying = self.replaying;
        self.replaying = true;
        for _ in 0..count.max(1) {
            for key in &keys {
                self.handle_key(key);
            }
        }
        self.replaying = was_replaying;
    }

    fn start_search(&mut self, direction: Direction) {
        self.minibuffer = Some(Minibuffer::search(direction));
        self.message.clear();
    }

    fn start_ex(&mut self) {
        let initial = self
            .visual_anchor
            .map_or_else(String::new, |_| "'<,'>".to_owned());
        self.minibuffer = Some(Minibuffer::ex(initial));
        self.message.clear();
    }

    fn handle_minibuffer_key(&mut self, key: &Key) {
        match &key.value {
            KeyValue::Named(name) if name == "escape" => {
                self.minibuffer = None;
                self.message.clear();
            }
            KeyValue::Named(name) if name == "backspace" => {
                if let Some(minibuffer) = &mut self.minibuffer {
                    minibuffer.text.pop();
                }
            }
            KeyValue::Named(name) if name == "enter" => {
                let Some(minibuffer) = self.minibuffer.take() else {
                    return;
                };
                match minibuffer.kind {
                    MinibufferKind::Search(direction) => {
                        self.submit_search(minibuffer.text, direction)
                    }
                    MinibufferKind::Ex => self.submit_ex(&minibuffer.text),
                }
            }
            KeyValue::Named(name) if name == "tab" => {
                let cwd = self.working_directory();
                let completion = self
                    .minibuffer
                    .as_mut()
                    .map_or(ShellPathCompletion::None, |minibuffer| {
                        minibuffer.complete_shell_path(&cwd)
                    });
                self.message = match completion {
                    ShellPathCompletion::None => "No file completion".to_owned(),
                    ShellPathCompletion::Completed => String::new(),
                    ShellPathCompletion::Candidates(candidates) => {
                        let visible = candidates
                            .into_iter()
                            .take(8)
                            .collect::<Vec<_>>()
                            .join("  ");
                        format!("Matches: {visible}")
                    }
                };
            }
            KeyValue::Named(name) if name == "u" && key.modifiers.control => {
                if let Some(minibuffer) = &mut self.minibuffer {
                    minibuffer.text.clear();
                }
            }
            KeyValue::Character(text)
                if !key.modifiers.control && !key.modifiers.alt && !key.modifiers.super_key =>
            {
                if let Some(minibuffer) = &mut self.minibuffer {
                    minibuffer.text.push_str(text);
                }
            }
            _ => {}
        }
    }

    fn submit_search(&mut self, query: String, direction: Direction) {
        let search = if query.is_empty() {
            self.last_search.clone().map(|mut search| {
                search.direction = direction;
                search
            })
        } else {
            Some(Search {
                query,
                direction,
                whole_word: false,
            })
        };
        if let Some(search) = search {
            self.last_search = Some(search.clone());
            self.execute_search(search, 1);
        } else {
            self.message = "No previous search".to_owned();
        }
    }

    fn submit_ex(&mut self, input: &str) {
        match ex::parse(input) {
            Ok(command) => self.execute_ex(command),
            Err(error) => self.message = error.to_string(),
        }
    }

    fn repeat_search(&mut self, reverse: bool, count: u32) {
        let Some(mut search) = self.last_search.clone() else {
            self.message = "No previous search".to_owned();
            return;
        };
        if reverse {
            search.direction = opposite(search.direction);
        }
        self.execute_search(search, count);
    }

    fn search_word(&mut self, direction: Direction, count: u32) {
        let Some(query) = search::word_under_cursor(&self.buffer, self.cursor.position) else {
            self.message = "No word under cursor".to_owned();
            return;
        };
        let search = Search {
            query,
            direction,
            whole_word: true,
        };
        self.last_search = Some(search.clone());
        self.execute_search(search, count);
    }

    fn execute_search(&mut self, search: Search, count: u32) {
        let origin = self.cursor.position;
        let Some(destination) = search::find(&self.buffer, origin, &search, count) else {
            self.message = format!("Pattern not found: {}", search.query);
            return;
        };
        self.jump_list.record(origin, destination);
        self.move_to_position(destination);
        self.message.clear();
    }

    fn jump_to_mark(&mut self, mark: char, linewise: bool) {
        let Some(mut destination) = self.marks.get(&mark).copied() else {
            self.message = format!("Mark not set: {mark}");
            return;
        };
        destination.line = destination
            .line
            .min(self.buffer.len_lines().saturating_sub(1));
        if linewise {
            destination.character = self.first_non_blank(destination.line);
        }
        destination.character = destination
            .character
            .min(self.maximum_character(destination.line));
        let origin = self.cursor.position;
        self.jump_list.record(origin, destination);
        self.move_to_position(destination);
        self.message.clear();
    }

    fn jump_older(&mut self, count: u32) {
        if let Some(position) = self.jump_list.older(count) {
            self.move_to_position(position);
        } else {
            self.message = "At oldest jump".to_owned();
        }
    }

    fn jump_newer(&mut self, count: u32) {
        if let Some(position) = self.jump_list.newer(count) {
            self.move_to_position(position);
        } else {
            self.message = "At newest jump".to_owned();
        }
    }

    fn change_older(&mut self, count: u32) {
        if let Some(position) = self.change_list.older(count) {
            self.move_to_position(position);
        } else {
            self.message = "At oldest change".to_owned();
        }
    }

    fn change_newer(&mut self, count: u32) {
        if let Some(position) = self.change_list.newer(count) {
            self.move_to_position(position);
        } else {
            self.message = "At newest change".to_owned();
        }
    }

    fn move_to_position(&mut self, mut position: BufferPosition) {
        position.line = position.line.min(self.buffer.len_lines().saturating_sub(1));
        position.character = position
            .character
            .min(self.maximum_character(position.line));
        self.cursor.position = position;
        self.cursor.desired_character = None;
        self.ensure_cursor_visible();
    }

    fn first_non_blank(&self, line: usize) -> usize {
        self.buffer
            .line_text(line)
            .unwrap_or_default()
            .chars()
            .take_while(|character| character.is_whitespace())
            .count()
            .min(self.maximum_character(line))
    }

    fn execute_ex(&mut self, command: ExCommand) {
        if self.is_read_only_special()
            && matches!(
                &command,
                ExCommand::Write { .. }
                    | ExCommand::Substitute { .. }
                    | ExCommand::Global { .. }
                    | ExCommand::Delete { .. }
            )
        {
            self.message = self.read_only_message().to_owned();
            return;
        }
        match command {
            ExCommand::GoTo(address) => match self.resolve_address(address) {
                Ok(line) => {
                    let origin = self.cursor.position;
                    let destination = BufferPosition {
                        line,
                        character: self.first_non_blank(line),
                    };
                    self.jump_list.record(origin, destination);
                    self.move_to_position(destination);
                    self.leave_visual_after_ex();
                }
                Err(error) => self.message = error,
            },
            ExCommand::Write {
                range,
                path,
                force,
                quit,
            } => {
                if !matches!(range, RangeSpec::Default | RangeSpec::Whole) {
                    self.message = "Ranged :write is not supported".to_owned();
                    return;
                }
                let saved = self.write_ex(path.as_deref(), force);
                if saved && quit {
                    self.return_to_dired(true);
                }
            }
            ExCommand::Quit { force } => self.return_to_dired(force),
            ExCommand::Edit { path, force } => self.edit_ex(path.as_deref(), force),
            ExCommand::Substitute {
                range,
                substitution,
            } => match self.resolve_range(&range, false) {
                Ok((start, end)) => {
                    let lines: Vec<usize> = (start..=end).collect();
                    self.apply_substitution(&lines, &substitution);
                    self.leave_visual_after_ex();
                }
                Err(error) => self.message = error,
            },
            ExCommand::Global {
                range,
                pattern,
                case_insensitive,
                invert,
                action,
            } => self.execute_global(&range, &pattern, case_insensitive, invert, action),
            ExCommand::Delete { range } => match self.resolve_range(&range, false) {
                Ok((start, end)) => {
                    self.delete_lines(&(start..=end).collect::<Vec<_>>());
                    self.leave_visual_after_ex();
                }
                Err(error) => self.message = error,
            },
            ExCommand::Shell {
                command,
                open_immediately,
            } => {
                if !self.is_read_only_special() && self.buffer.is_modified() {
                    self.message =
                        "Write or discard changes before running a shell command".to_owned();
                    return;
                }
                let request = ShellRequest {
                    id: self.next_shell_request_id,
                    command,
                    cwd: self.working_directory(),
                };
                self.next_shell_request_id = self.next_shell_request_id.wrapping_add(1).max(1);
                self.message = format!("Running: {}", request.command);
                if open_immediately {
                    let (buffer, output) = OutputState::running(&request);
                    self.replace_buffer(buffer, None, Some(output));
                    self.cursor.position.line = 1.min(self.buffer.len_lines().saturating_sub(1));
                }
                self.request = Some(EditorRequest::RunShell {
                    request,
                    open_immediately,
                });
            }
        }
    }

    fn is_read_only_special(&self) -> bool {
        self.dired.is_some() || self.output.is_some()
    }

    fn read_only_message(&self) -> &'static str {
        if self.dired.is_some() {
            "Dired buffers are read-only"
        } else {
            "Output buffers are read-only"
        }
    }

    fn working_directory(&self) -> PathBuf {
        if let Some(dired) = &self.dired {
            dired.directory().to_path_buf()
        } else if let Some(output) = &self.output {
            output.cwd().to_path_buf()
        } else {
            self.buffer
                .path()
                .and_then(containing_directory)
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."))
        }
    }

    fn execute_output_target(&mut self) {
        let target = self
            .output
            .as_ref()
            .and_then(|output| output.target(self.cursor.position.line).cloned());
        let cwd = self
            .output
            .as_ref()
            .map(|output| output.cwd().to_path_buf());
        let Some(JumpTarget { path, line, column }) = target else {
            self.message = "No path target on this line".to_owned();
            return;
        };
        match Buffer::open(&path) {
            Ok(buffer) => {
                self.replace_buffer(buffer, None, None);
                self.output_return = cwd;
                self.cursor.position.line = line.min(self.buffer.len_lines().saturating_sub(1));
                self.cursor.position.character =
                    column.min(self.maximum_character(self.cursor.position.line));
                self.ensure_cursor_visible();
                self.message = format!("Edited {}", path.display());
            }
            Err(error) => self.message = format!("Could not open {}: {error}", path.display()),
        }
    }

    fn execute_dired(&mut self, command: DiredCommand) {
        let origin_directory = self
            .dired
            .as_ref()
            .map(|dired| dired.directory().to_path_buf());
        let target = match command {
            DiredCommand::Open => self
                .dired
                .as_ref()
                .and_then(|dired| dired.entry(self.cursor.position.line))
                .map(|entry| entry.path.clone()),
            DiredCommand::Parent => self.dired.as_ref().map(|dired| {
                dired
                    .directory()
                    .parent()
                    .unwrap_or_else(|| dired.directory())
                    .to_path_buf()
            }),
        };
        let Some(target) = target else {
            self.message = "No Dired entry on this line".to_owned();
            return;
        };
        if target.is_dir() {
            self.open_dired(&target);
        } else {
            match Buffer::open(&target) {
                Ok(buffer) => {
                    self.replace_buffer(buffer, None, None);
                    if let Some(directory) = origin_directory {
                        self.dired_return = Some(DiredReturn {
                            directory,
                            selected_path: target.clone(),
                        });
                    }
                    self.message = format!("Edited {}", target.display());
                }
                Err(error) => {
                    self.message = format!("Could not open {}: {error}", target.display())
                }
            }
        }
    }

    fn open_dired(&mut self, directory: &Path) {
        self.open_dired_at(directory, None);
    }

    fn open_dired_at(&mut self, directory: &Path, selected_path: Option<&Path>) {
        match DiredState::load(directory) {
            Ok((buffer, dired)) => {
                let line = selected_path.and_then(|path| dired.line_for_path(path));
                self.replace_buffer(buffer, Some(dired), None);
                if let Some(line) = line {
                    self.cursor.position.line = line;
                    self.ensure_cursor_visible();
                }
                self.message = format!("Dired {}", directory.display());
            }
            Err(error) => {
                self.message = format!("Could not open directory {}: {error}", directory.display())
            }
        }
    }

    fn return_to_dired(&mut self, force: bool) {
        if self.dired.is_some() {
            self.request = Some(EditorRequest::Close);
            return;
        }
        if !self.is_read_only_special() && self.buffer.is_modified() && !force {
            self.message = "No write since last change (add ! to override)".to_owned();
            return;
        }
        if let Some(context) = self.dired_return.clone() {
            self.open_dired_at(&context.directory, Some(&context.selected_path));
            return;
        }
        if let Some(directory) = self.output_return.clone().or_else(|| {
            self.output
                .as_ref()
                .map(|output| output.cwd().to_path_buf())
        }) {
            self.open_dired(&directory);
            return;
        }
        let directory = self.buffer.path().and_then(containing_directory);
        let Some(directory) = directory else {
            self.message = "Could not determine a directory for Dired".to_owned();
            return;
        };
        self.open_dired(&directory);
    }

    fn write_ex(&mut self, path: Option<&str>, force: bool) -> bool {
        let mut written_name = self.buffer.display_name();
        let result = if let Some(path) = path {
            let path = PathBuf::from(path);
            let different_file = self.buffer.path().is_none_or(|current| current != path);
            if different_file && path.exists() && !force {
                self.message = format!("File exists: {} (add ! to override)", path.display());
                return false;
            }
            written_name = path.display().to_string();
            if self.buffer.path().is_some() && different_file {
                self.buffer.write_to(&path)
            } else {
                self.buffer.save_as(path)
            }
        } else {
            self.buffer.save()
        };
        match result {
            Ok(()) => {
                self.message = format!("Wrote {written_name}");
                true
            }
            Err(error) => {
                self.message = format!("Write failed: {error}");
                false
            }
        }
    }

    fn edit_ex(&mut self, path: Option<&str>, force: bool) {
        if !self.is_read_only_special() && self.buffer.is_modified() && !force {
            self.message = "No write since last change (add ! to override)".to_owned();
            return;
        }
        let path = path
            .map(PathBuf::from)
            .or_else(|| self.buffer.path().map(Path::to_path_buf));
        let Some(path) = path else {
            self.message = "No file name".to_owned();
            return;
        };
        if path.is_dir() {
            match DiredState::load(&path) {
                Ok((buffer, dired)) => {
                    self.replace_buffer(buffer, Some(dired), None);
                    self.message = format!("Dired {}", path.display());
                }
                Err(error) => self.message = format!("Edit failed: {error}"),
            }
            return;
        }
        match Buffer::open(&path) {
            Ok(buffer) => {
                self.replace_buffer(buffer, None, None);
                self.message = format!("Edited {}", path.display());
            }
            Err(error) => self.message = format!("Edit failed: {error}"),
        }
    }

    fn replace_buffer(
        &mut self,
        buffer: Buffer,
        dired: Option<DiredState>,
        output: Option<OutputState>,
    ) {
        self.buffer = buffer;
        self.dired = dired;
        self.dired_return = None;
        self.output = output;
        self.output_return = None;
        self.cursor = Cursor::default();
        self.viewport = Viewport::default();
        self.vim = VimEngine::default();
        self.last_find = None;
        self.visual_anchor = None;
        self.block_insert = None;
        self.insert_session_changed = false;
        self.pending_change_keys.clear();
        self.last_change_keys.clear();
        self.change_list = ChangeList::default();
        self.jump_list = JumpList::default();
        self.marks.clear();
        self.last_search = None;
        self.last_ex_pattern = None;
    }

    fn resolve_range(
        &self,
        range: &RangeSpec,
        default_whole: bool,
    ) -> Result<(usize, usize), String> {
        let last = self.buffer.logical_len_lines().saturating_sub(1);
        match range {
            RangeSpec::Default if default_whole => Ok((0, last)),
            RangeSpec::Default => Ok((self.cursor.position.line, self.cursor.position.line)),
            RangeSpec::Whole => Ok((0, last)),
            RangeSpec::Addresses {
                start,
                end,
                separator,
            } => {
                let original_current = self.cursor.position.line;
                let start = self.resolve_address_at(*start, original_current)?;
                let second_current = if *separator == Some(RangeSeparator::Semicolon) {
                    start
                } else {
                    original_current
                };
                let end = end.map_or(Ok(start), |address| {
                    self.resolve_address_at(address, second_current)
                })?;
                if start > end {
                    Err("Backward range".to_owned())
                } else {
                    Ok((start, end))
                }
            }
        }
    }

    fn resolve_address(&self, address: Address) -> Result<usize, String> {
        self.resolve_address_at(address, self.cursor.position.line)
    }

    fn resolve_address_at(&self, address: Address, current: usize) -> Result<usize, String> {
        let last = self.buffer.logical_len_lines().saturating_sub(1);
        match address {
            Address::Current => Ok(current.min(last)),
            Address::Last => Ok(last),
            Address::Line(line) if line <= self.buffer.logical_len_lines() => Ok(line - 1),
            Address::Line(line) => Err(format!("Line number out of range: {line}")),
            Address::Mark(mark) => self
                .marks
                .get(&mark)
                .map(|position| position.line.min(last))
                .ok_or_else(|| format!("Mark not set: {mark}")),
            Address::VisualStart => self
                .visual_line_range()
                .map(|(start, _)| start)
                .ok_or_else(|| "No Visual selection".to_owned()),
            Address::VisualEnd => self
                .visual_line_range()
                .map(|(_, end)| end)
                .ok_or_else(|| "No Visual selection".to_owned()),
        }
    }

    fn visual_line_range(&self) -> Option<(usize, usize)> {
        let anchor = self.visual_anchor?;
        Some((
            anchor.line.min(self.cursor.position.line),
            anchor.line.max(self.cursor.position.line),
        ))
    }

    fn execute_global(
        &mut self,
        range: &RangeSpec,
        pattern: &str,
        case_insensitive: bool,
        invert: bool,
        action: GlobalAction,
    ) {
        let (start, end) = match self.resolve_range(range, true) {
            Ok(range) => range,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        let (regex, _) = match self.compile_ex_pattern(pattern, case_insensitive) {
            Ok(compiled) => compiled,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        let lines: Vec<usize> = (start..=end)
            .filter(|line| {
                let matches = self
                    .buffer
                    .line_text(*line)
                    .is_some_and(|text| regex.is_match(&text));
                matches != invert
            })
            .collect();
        match action {
            GlobalAction::Count => {
                self.message = format!("{} matching lines", lines.len());
            }
            GlobalAction::Delete => self.delete_lines(&lines),
            GlobalAction::Substitute(substitution) => {
                self.apply_substitution(&lines, &substitution)
            }
        }
        self.leave_visual_after_ex();
    }

    fn apply_substitution(&mut self, lines: &[usize], substitution: &Substitute) {
        let (regex, pattern) =
            match self.compile_ex_pattern(&substitution.pattern, substitution.case_insensitive) {
                Ok(compiled) => compiled,
                Err(error) => {
                    self.message = error;
                    return;
                }
            };
        let mut edits = Vec::new();
        let mut substitutions = 0;
        for &line in lines {
            let Some(text) = self.buffer.line_text(line) else {
                continue;
            };
            let count = if substitution.global {
                regex.find_iter(&text).count()
            } else {
                usize::from(regex.is_match(&text))
            };
            if count == 0 {
                continue;
            }
            let replacement = if substitution.global {
                regex.replace_all(&text, substitution.replacement.as_str())
            } else {
                regex.replace(&text, substitution.replacement.as_str())
            };
            let Some(start) = self.buffer.line_start_offset(line) else {
                continue;
            };
            edits.push((
                start,
                CharOffset(start.0 + text.chars().count()),
                replacement.into_owned(),
            ));
            substitutions += count;
        }
        if edits.is_empty() {
            self.message = format!("Pattern not found: {pattern}");
            return;
        }
        self.buffer.begin_transaction();
        for (start, end, replacement) in edits.into_iter().rev() {
            let _ = self.buffer.remove(start, end);
            let _ = self.buffer.insert(start, &replacement);
        }
        self.buffer.commit_transaction();
        self.message = format!("{substitutions} substitutions");
    }

    fn compile_ex_pattern(
        &mut self,
        pattern: &str,
        case_insensitive: bool,
    ) -> Result<(Regex, String), String> {
        let pattern = if pattern.is_empty() {
            self.last_ex_pattern
                .clone()
                .or_else(|| {
                    self.last_search
                        .as_ref()
                        .map(|search| regex::escape(&search.query))
                })
                .ok_or_else(|| "No previous pattern".to_owned())?
        } else {
            pattern.to_owned()
        };
        let regex = compile_regex(&pattern, case_insensitive)?;
        self.last_ex_pattern = Some(pattern.clone());
        Ok((regex, pattern))
    }

    fn delete_lines(&mut self, lines: &[usize]) {
        if lines.is_empty() {
            self.message = "No matching lines".to_owned();
            return;
        }
        let mut deleted_text = lines
            .iter()
            .filter_map(|line| self.buffer.line_text(*line))
            .collect::<Vec<_>>()
            .join("\n");
        deleted_text.push('\n');
        self.registers.record_delete(
            RegisterName::Unnamed,
            RegisterValue {
                text: deleted_text,
                shape: SelectionShape::LineWise,
            },
        );
        self.buffer.begin_transaction();
        for &line in lines.iter().rev() {
            if line >= self.buffer.len_lines() {
                continue;
            }
            let Some((start, end)) = self.line_delete_bounds(line) else {
                continue;
            };
            let _ = self.buffer.remove(start, end);
        }
        self.buffer.commit_transaction();
        self.cursor.position.line = lines[0].min(self.buffer.len_lines().saturating_sub(1));
        self.cursor.position.character = 0;
        self.clamp_cursor_to_mode();
        self.ensure_cursor_visible();
        self.message = format!("{} lines deleted", lines.len());
    }

    fn line_delete_bounds(&self, line: usize) -> Option<(CharOffset, CharOffset)> {
        let start = self.buffer.line_start_offset(line)?;
        if line + 1 < self.buffer.len_lines() {
            return Some((start, self.buffer.line_start_offset(line + 1)?));
        }
        if line == 0 {
            return Some((start, CharOffset(self.buffer.len_chars())));
        }
        let mut adjusted = start.0;
        if adjusted > 0 && self.buffer.char_at(CharOffset(adjusted - 1)) == Some('\n') {
            adjusted -= 1;
            if adjusted > 0 && self.buffer.char_at(CharOffset(adjusted - 1)) == Some('\r') {
                adjusted -= 1;
            }
        }
        Some((CharOffset(adjusted), CharOffset(self.buffer.len_chars())))
    }

    fn leave_visual_after_ex(&mut self) {
        if self.visual_anchor.is_some() {
            self.visual_anchor = None;
            self.vim.leave_transient_mode();
            self.clamp_cursor_to_mode();
        }
    }

    fn track_completed_transaction(&mut self, transactions_before: usize) {
        if self.buffer.completed_transaction_count() <= transactions_before {
            return;
        }
        if let Some(position) = self
            .buffer
            .last_completed_transaction_start()
            .and_then(|offset| self.buffer.offset_to_position(offset).ok())
        {
            self.change_list.record(position);
        }
        if !self.replaying && !self.pending_change_keys.is_empty() {
            self.last_change_keys = std::mem::take(&mut self.pending_change_keys);
        }
    }

    fn save(&mut self) {
        if self.is_read_only_special() {
            self.message = self.read_only_message().to_owned();
            return;
        }
        self.write_ex(None, false);
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

fn is_plain_named_key(key: &Key, expected: &str) -> bool {
    !key.modifiers.control
        && !key.modifiers.alt
        && !key.modifiers.super_key
        && matches!(&key.value, KeyValue::Named(name) if name == expected)
}

fn containing_directory(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
}

fn is_mutating_command(command: Command) -> bool {
    match command {
        Command::EnterInsert(_)
        | Command::DeleteCharacters { .. }
        | Command::EnterReplace
        | Command::ReplaceCharacter { .. }
        | Command::BeginBlockInsert { .. }
        | Command::Paste { .. }
        | Command::RepeatChange { .. } => true,
        Command::Operate(command) => command.operator != Operator::Yank,
        Command::OperateSelection { operator, .. } => operator != Operator::Yank,
        _ => false,
    }
}

fn compile_regex(pattern: &str, case_insensitive: bool) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|error| format!("Invalid pattern: {error}"))
}

fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::Forward => Direction::Backward,
        Direction::Backward => Direction::Forward,
    }
}

fn starts_change(key: &Key) -> bool {
    matches!(
        &key.value,
        KeyValue::Character(value)
            if value.chars().next().is_some_and(|character| {
                matches!(
                    character,
                    'i' | 'a' | 'I' | 'A' | 'o' | 'O' | 'd' | 'c' | 'x' | 'X'
                        | 'p' | 'P' | 'r' | 'R' | '>' | '<' | '='
                )
            })
    )
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
    use std::{fs, process};

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

    #[test]
    fn ex_substitution_respects_ranges_and_is_one_undo_step() {
        let mut editor = editor_with_text("cat cat\ncat\ndog");

        execute_ex_keys(&mut editor, "1,2s/cat/fox/g");

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("fox fox"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("fox"));
        assert_eq!(editor.buffer.line_text(2).as_deref(), Some("dog"));
        assert_eq!(editor.buffer.completed_transaction_count(), 1);
        assert_eq!(editor.message(), "3 substitutions");

        type_keys(&mut editor, "u");
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("cat cat"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("cat"));
    }

    #[test]
    fn semicolon_ranges_resolve_dot_from_the_first_address() {
        let mut editor = editor_with_text("one\ntwo\nthree");
        type_keys(&mut editor, "G");

        execute_ex_keys(&mut editor, "1;.d");

        assert_eq!(
            editor
                .buffer
                .text(CharOffset(0), CharOffset(editor.buffer.len_chars()))
                .as_deref(),
            Some("two\nthree")
        );
    }

    #[test]
    fn empty_ex_patterns_reuse_history_or_fail_without_mutating() {
        let mut missing = editor_with_text("cat");
        execute_ex_keys(&mut missing, "s//dog/g");
        assert_eq!(missing.buffer.line_text(0).as_deref(), Some("cat"));
        assert_eq!(missing.message(), "No previous pattern");
        assert_eq!(missing.buffer.completed_transaction_count(), 0);

        let mut repeated = editor_with_text("cat cat");
        execute_ex_keys(&mut repeated, "s/cat/dog/");
        execute_ex_keys(&mut repeated, "s//fox/");
        assert_eq!(repeated.buffer.line_text(0).as_deref(), Some("dog fox"));
    }

    #[test]
    fn visual_ex_range_is_prefilled_and_applied() {
        let mut editor = editor_with_text("cat\ncat\ncat");

        type_keys(&mut editor, "Vj:");
        assert_eq!(editor.prompt_text().as_deref(), Some(":'<,'>"));
        type_keys(&mut editor, "s/cat/dog/");
        editor.handle_key(&named("enter"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("dog"));
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("dog"));
        assert_eq!(editor.buffer.line_text(2).as_deref(), Some("cat"));
        assert_eq!(editor.mode(), Mode::Normal);
    }

    #[test]
    fn global_commands_delete_and_substitute_matching_lines() {
        let mut deletion = editor_with_text("keep\nerror one\nkeep\nerror two");
        execute_ex_keys(&mut deletion, "g/^error/d");
        assert_eq!(
            deletion
                .buffer
                .text(CharOffset(0), CharOffset(deletion.buffer.len_chars()))
                .as_deref(),
            Some("keep\nkeep")
        );
        assert_eq!(deletion.buffer.completed_transaction_count(), 1);

        let mut substitution = editor_with_text("keep\nerror one\nerror two");
        execute_ex_keys(&mut substitution, "g/^error/s/error/fixed/");
        assert_eq!(
            substitution.buffer.line_text(1).as_deref(),
            Some("fixed one")
        );
        assert_eq!(
            substitution.buffer.line_text(2).as_deref(),
            Some("fixed two")
        );
    }

    #[test]
    fn ex_delete_uses_logical_lines_and_updates_registers() {
        let mut lf = editor_with_text("one\ntwo\n");
        execute_ex_keys(&mut lf, "$d");
        assert_eq!(
            lf.buffer
                .text(CharOffset(0), CharOffset(lf.buffer.len_chars()))
                .as_deref(),
            Some("one\n")
        );
        assert_eq!(
            lf.registers.unnamed(),
            Some(&RegisterValue {
                text: "two\n".to_owned(),
                shape: SelectionShape::LineWise
            })
        );

        let mut crlf = editor_with_text("one\r\ntwo\r\n");
        execute_ex_keys(&mut crlf, "$d");
        assert_eq!(
            crlf.buffer
                .text(CharOffset(0), CharOffset(crlf.buffer.len_chars()))
                .as_deref(),
            Some("one\r\n")
        );
    }

    #[test]
    fn writing_an_alternate_path_does_not_retarget_a_named_buffer() {
        let directory = temporary_directory("alternate-write");
        fs::create_dir(&directory).expect("create test directory");
        let current = directory.join("current.txt");
        let alternate = directory.join("alternate.txt");
        fs::write(&current, "one").expect("write current fixture");
        fs::write(&alternate, "old").expect("write alternate fixture");

        let mut editor = Editor::open(&current);
        type_keys(&mut editor, "A!");
        editor.handle_key(&named("escape"));
        execute_ex_keys(&mut editor, &format!("w! {}", alternate.display()));

        assert_eq!(
            fs::read_to_string(&alternate).expect("read alternate"),
            "one!"
        );
        assert_eq!(editor.buffer.path(), Some(current.as_path()));
        assert!(editor.buffer.is_modified());

        execute_ex_keys(&mut editor, "w");
        assert_eq!(fs::read_to_string(&current).expect("read current"), "one!");
        assert!(!editor.buffer.is_modified());

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn ranged_lifecycle_commands_do_not_close_or_replace_the_buffer() {
        let mut editor = editor_with_text("one");

        execute_ex_keys(&mut editor, "%q!");
        assert_eq!(editor.take_request(), None);
        assert_eq!(editor.message(), "No range allowed");
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one"));
    }

    #[test]
    fn ex_write_edit_and_quit_protect_user_changes() {
        let directory = temporary_directory("ex-files");
        fs::create_dir(&directory).expect("create test directory");
        let first = directory.join("first.txt");
        let second = directory.join("second.txt");
        fs::write(&second, "second").expect("write second fixture");

        let mut editor = Editor::scratch();
        type_keys(&mut editor, "ihello");
        editor.handle_key(&named("escape"));
        execute_ex_keys(&mut editor, &format!("w {}", first.display()));
        assert_eq!(
            fs::read_to_string(&first).expect("read saved file"),
            "hello"
        );
        assert!(!editor.buffer.is_modified());

        type_keys(&mut editor, "A!");
        editor.handle_key(&named("escape"));
        execute_ex_keys(&mut editor, &format!("e {}", second.display()));
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("hello!"));
        assert!(editor.message().contains("No write since last change"));

        execute_ex_keys(&mut editor, &format!("e! {}", second.display()));
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("second"));
        assert!(!editor.buffer.is_modified());

        type_keys(&mut editor, "A!");
        editor.handle_key(&named("escape"));
        execute_ex_keys(&mut editor, "q");
        assert_eq!(editor.take_request(), None);
        execute_ex_keys(&mut editor, "q!");
        assert!(editor.is_dired());
        assert_eq!(editor.take_request(), None);
        execute_ex_keys(&mut editor, "q");
        assert_eq!(editor.take_request(), Some(EditorRequest::Close));

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn dired_opens_entries_and_returns_from_files_on_quit() {
        let directory = temporary_directory("dired-open");
        fs::create_dir(&directory).expect("create test directory");
        fs::create_dir(directory.join("folder")).expect("create child directory");
        fs::write(directory.join("file.txt"), "contents").expect("write file fixture");

        let mut editor = Editor::dired(&directory);
        assert!(editor.is_dired());
        assert!(
            editor
                .display_name()
                .contains(directory.to_string_lossy().as_ref())
        );

        type_keys(&mut editor, "2j");
        editor.handle_key(&named("enter"));
        assert!(!editor.is_dired());
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("contents"));

        execute_ex_keys(&mut editor, "q");
        assert!(editor.is_dired());
        assert_eq!(editor.take_request(), None);
        assert_eq!(editor.cursor.position.line, 2);
        assert_eq!(
            editor
                .dired
                .as_ref()
                .and_then(|dired| dired.entry(editor.cursor.position.line))
                .map(|entry| entry.path.as_path()),
            Some(directory.join("file.txt").as_path())
        );

        execute_ex_keys(&mut editor, "q");
        assert_eq!(editor.take_request(), Some(EditorRequest::Close));

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn dired_backspace_opens_the_parent_and_mutations_are_blocked() {
        let directory = temporary_directory("dired-parent");
        let child = directory.join("child");
        fs::create_dir_all(&child).expect("create child directory");
        fs::write(child.join("file.txt"), "contents").expect("write file fixture");

        let mut editor = Editor::dired(&child);
        let original = editor
            .buffer
            .text(CharOffset(0), CharOffset(editor.buffer.len_chars()))
            .expect("Dired text");
        type_keys(&mut editor, "x");
        assert_eq!(editor.message(), "Dired buffers are read-only");
        assert_eq!(
            editor
                .buffer
                .text(CharOffset(0), CharOffset(editor.buffer.len_chars()))
                .as_deref(),
            Some(original.as_str())
        );

        execute_ex_keys(&mut editor, "%s/file/changed/g");
        assert_eq!(editor.message(), "Dired buffers are read-only");

        editor.handle_key(&named("backspace"));
        assert!(editor.is_dired());
        assert!(
            editor
                .display_name()
                .contains(directory.to_string_lossy().as_ref())
        );

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn write_quit_saves_then_returns_to_dired() {
        let directory = temporary_directory("dired-write-quit");
        fs::create_dir(&directory).expect("create test directory");
        let path = directory.join("file.txt");
        fs::write(&path, "one").expect("write file fixture");

        let mut editor = Editor::open(&path);
        type_keys(&mut editor, "A!");
        editor.handle_key(&named("escape"));
        execute_ex_keys(&mut editor, "wq");

        assert_eq!(fs::read_to_string(&path).expect("read saved file"), "one!");
        assert!(editor.is_dired());
        assert_eq!(editor.take_request(), None);

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn bare_relative_file_paths_fall_back_to_the_current_directory() {
        assert_eq!(
            containing_directory(Path::new("file.txt")),
            std::env::current_dir().ok()
        );
    }

    #[test]
    fn modified_file_rejects_shell_commands() {
        let directory = temporary_directory("shell-modified");
        fs::create_dir(&directory).expect("create test directory");
        let path = directory.join("notes.txt");
        fs::write(&path, "one").expect("write file fixture");

        let mut editor = Editor::open(&path);
        type_keys(&mut editor, "A!");
        editor.handle_key(&named("escape"));
        execute_ex_keys(&mut editor, "!printf output");

        assert_eq!(editor.take_request(), None);
        assert_eq!(
            editor.message(),
            "Write or discard changes before running a shell command"
        );
        assert_eq!(editor.buffer.path(), Some(path.as_path()));
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one!"));

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn shell_completion_does_not_replace_a_buffer_modified_while_running() {
        let directory = temporary_directory("shell-stale-completion");
        fs::create_dir(&directory).expect("create test directory");
        let path = directory.join("notes.txt");
        fs::write(&path, "one").expect("write file fixture");

        let mut editor = Editor::open(&path);
        execute_ex_keys(&mut editor, "!printf output");
        let request = match editor.take_request() {
            Some(EditorRequest::RunShell {
                request,
                open_immediately: false,
            }) => request,
            other => panic!("expected delayed shell request, got {other:?}"),
        };
        type_keys(&mut editor, "A!");
        editor.handle_key(&named("escape"));

        editor.finish_shell(
            ShellCompletion {
                request,
                result: Ok(crate::shell::ShellResult {
                    output: "output".to_owned(),
                    exit_code: Some(0),
                    success: true,
                }),
            },
            false,
        );

        assert!(!editor.is_output());
        assert_eq!(editor.buffer.path(), Some(path.as_path()));
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one!"));
        assert_eq!(
            editor.message(),
            "Shell finished, but output was not opened because the buffer changed"
        );

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn run_opens_a_matching_output_buffer_before_completion() {
        let directory = temporary_directory("shell-immediate-output");
        fs::create_dir(&directory).expect("create test directory");

        let mut editor = Editor::dired(&directory);
        execute_ex_keys(&mut editor, "run printf output");

        assert!(editor.is_output());
        assert!(editor.display_name().contains("[running]"));
        assert_eq!(
            editor.buffer.line_text(0).as_deref(),
            Some("$ printf output")
        );
        let request = match editor.take_request() {
            Some(EditorRequest::RunShell {
                request,
                open_immediately: true,
            }) => request,
            other => panic!("expected immediate shell request, got {other:?}"),
        };
        editor.append_shell_output(request.id, "output");
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("output"));
        editor.finish_shell(
            ShellCompletion {
                request,
                result: Ok(crate::shell::ShellResult {
                    output: "output".to_owned(),
                    exit_code: Some(0),
                    success: true,
                }),
            },
            true,
        );

        assert!(editor.is_output());
        assert!(editor.display_name().contains("exit 0, ok"));
        assert_eq!(
            editor.buffer.line_text(0).as_deref(),
            Some("$ printf output")
        );
        assert_eq!(editor.buffer.line_text(1).as_deref(), Some("output"));

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn leaving_a_running_output_ignores_its_late_completion() {
        let directory = temporary_directory("shell-immediate-stale");
        fs::create_dir(&directory).expect("create test directory");

        let mut editor = Editor::dired(&directory);
        execute_ex_keys(&mut editor, "run printf output");
        let request = match editor.take_request() {
            Some(EditorRequest::RunShell {
                request,
                open_immediately: true,
            }) => request,
            other => panic!("expected immediate shell request, got {other:?}"),
        };
        editor.append_shell_output(request.id, "partial output");
        execute_ex_keys(&mut editor, "q");
        assert!(editor.is_dired());

        editor.finish_shell(
            ShellCompletion {
                request,
                result: Ok(crate::shell::ShellResult {
                    output: "output".to_owned(),
                    exit_code: Some(0),
                    success: true,
                }),
            },
            true,
        );

        assert!(editor.is_dired());
        assert!(!editor.is_output());

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn shell_output_opens_targets_and_returns_to_its_working_directory() {
        let directory = temporary_directory("shell-output");
        fs::create_dir(&directory).expect("create test directory");
        let path = directory.join("main.rs");
        fs::write(&path, "one\ntwo\nthree").expect("write target fixture");

        let mut editor = Editor::dired(&directory);
        execute_ex_keys(&mut editor, "!printf main.rs:3:2");
        let request = match editor.take_request() {
            Some(EditorRequest::RunShell {
                request,
                open_immediately: false,
            }) => request,
            other => panic!("expected delayed shell request, got {other:?}"),
        };
        assert_eq!(
            request.cwd,
            directory.canonicalize().expect("canonical directory")
        );
        editor.finish_shell(
            ShellCompletion {
                request,
                result: Ok(crate::shell::ShellResult {
                    output: "main.rs:3:2: error".to_owned(),
                    exit_code: Some(1),
                    success: false,
                }),
            },
            false,
        );

        assert!(editor.is_output());
        assert!(editor.display_name().contains("exit 1, failed"));
        assert_eq!(
            editor.buffer.line_text(0).as_deref(),
            Some("$ printf main.rs:3:2")
        );
        assert_eq!(editor.cursor.position.line, 1);
        type_keys(&mut editor, "x");
        assert_eq!(editor.message(), "Output buffers are read-only");
        editor.handle_key(&named("enter"));
        assert!(!editor.is_output());
        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 2,
                character: 1
            }
        );

        execute_ex_keys(&mut editor, "q");
        assert!(editor.is_dired());
        assert!(
            editor
                .display_name()
                .contains(directory.to_string_lossy().as_ref())
        );

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn output_enter_without_a_target_reports_an_error() {
        let request = ShellRequest {
            id: 1,
            command: "printf hello".to_owned(),
            cwd: std::env::current_dir().expect("current directory"),
        };
        let mut editor = Editor::scratch();
        editor.finish_shell(
            ShellCompletion {
                request,
                result: Ok(crate::shell::ShellResult {
                    output: "hello".to_owned(),
                    exit_code: Some(0),
                    success: true,
                }),
            },
            false,
        );

        editor.handle_key(&named("enter"));

        assert!(editor.is_output());
        assert_eq!(editor.message(), "No path target on this line");
    }

    #[test]
    fn ex_line_address_jumps_to_first_non_blank() {
        let mut editor = editor_with_text("one\n  two\nthree");

        execute_ex_keys(&mut editor, "2");

        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 1,
                character: 2
            }
        );
    }

    #[test]
    fn search_prompt_repeats_and_reverses_wrapping_searches() {
        let mut editor = editor_with_text("one two one three one");

        type_keys(&mut editor, "/one");
        assert_eq!(editor.prompt_text().as_deref(), Some("/one"));
        editor.handle_key(&named("enter"));
        assert_eq!(editor.cursor.position.character, 8);

        type_keys(&mut editor, "n");
        assert_eq!(editor.cursor.position.character, 18);
        type_keys(&mut editor, "N");
        assert_eq!(editor.cursor.position.character, 8);

        type_keys(&mut editor, "?two");
        editor.handle_key(&named("backspace"));
        type_keys(&mut editor, "o");
        editor.handle_key(&named("enter"));
        assert_eq!(editor.cursor.position.character, 4);

        type_keys(&mut editor, "/cancelled");
        editor.handle_key(&named("escape"));
        assert_eq!(editor.prompt_text(), None);
        assert_eq!(editor.cursor.position.character, 4);
    }

    #[test]
    fn visual_search_moves_the_active_selection_end() {
        let mut editor = editor_with_text("one two one");

        type_keys(&mut editor, "v/one");
        editor.handle_key(&named("enter"));

        assert_eq!(editor.mode(), Mode::VisualCharacter);
        assert_eq!(editor.cursor.position.character, 8);
        assert_eq!(editor.selection_columns(0), Some((0, 8)));
    }

    #[test]
    fn failed_navigation_commands_leave_an_actionable_message() {
        let mut editor = editor_with_text("one");

        type_keys(&mut editor, "n");
        assert_eq!(editor.message(), "No previous search");
        type_keys(&mut editor, "`a");
        assert_eq!(editor.message(), "Mark not set: a");
    }

    #[test]
    fn word_search_uses_keyword_boundaries_and_counts() {
        let mut editor = editor_with_text("item items item x item");

        type_keys(&mut editor, "*");
        assert_eq!(editor.cursor.position.character, 11);
        type_keys(&mut editor, "#");
        assert_eq!(editor.cursor.position.character, 0);
        type_keys(&mut editor, "2*");
        assert_eq!(editor.cursor.position.character, 18);
    }

    #[test]
    fn marks_support_exact_and_linewise_jumps() {
        let mut editor = editor_with_text("  alpha\nbeta\ngamma");

        type_keys(&mut editor, "4lmaG`a");
        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 0,
                character: 4
            }
        );

        type_keys(&mut editor, "G'a");
        assert_eq!(
            editor.cursor.position,
            BufferPosition {
                line: 0,
                character: 2
            }
        );
    }

    #[test]
    fn jump_list_traverses_searches_and_line_jumps() {
        let mut editor = editor_with_text("one\ntwo\nthree");

        type_keys(&mut editor, "G");
        assert_eq!(editor.cursor.position.line, 2);
        editor.handle_key(&control_named("o"));
        assert_eq!(editor.cursor.position.line, 0);
        editor.handle_key(&control_named("i"));
        assert_eq!(editor.cursor.position.line, 2);
    }

    #[test]
    fn change_list_tracks_committed_change_origins() {
        let mut editor = editor_with_text("one\ntwo\nthree");

        type_keys(&mut editor, "xjxjxggg;");
        assert_eq!(editor.cursor.position.line, 2);
        type_keys(&mut editor, "g;");
        assert_eq!(editor.cursor.position.line, 1);
        type_keys(&mut editor, "g,");
        assert_eq!(editor.cursor.position.line, 2);
    }

    #[test]
    fn undo_redo_and_branching_preserve_transactions() {
        let mut editor = editor_with_text("one");
        type_keys(&mut editor, "A!");
        editor.handle_key(&named("escape"));
        type_keys(&mut editor, "u");
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one"));
        editor.handle_key(&control_named("r"));
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one!"));

        type_keys(&mut editor, "uA?");
        editor.handle_key(&named("escape"));
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one?"));
        assert_eq!(editor.buffer.undo_branch_count(), 2);
        type_keys(&mut editor, "u");
        editor.handle_key(&control_named("r"));
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one?"));
    }

    #[test]
    fn redo_accepts_a_count() {
        let mut editor = editor_with_text("one");
        for value in ['!', '?', '.'] {
            type_keys(&mut editor, "A");
            editor.handle_key(&character(value));
            editor.handle_key(&named("escape"));
        }

        type_keys(&mut editor, "3u2");
        editor.handle_key(&control_named("r"));

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one!?"));
    }

    #[test]
    fn macros_record_play_and_repeat_with_counts() {
        let mut editor = editor_with_text("one");
        type_keys(&mut editor, "qaA!");
        editor.handle_key(&named("escape"));
        type_keys(&mut editor, "q2@a@@.");
        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("one!!!!!"));
    }

    #[test]
    fn q_in_insert_mode_is_recorded_instead_of_stopping_a_macro() {
        let mut editor = editor_with_text("one");
        type_keys(&mut editor, "qaiq");
        editor.handle_key(&named("escape"));
        type_keys(&mut editor, "q@a");

        assert_eq!(editor.buffer.line_text(0).as_deref(), Some("qqone"));
    }

    #[test]
    fn dot_repeats_insert_and_operator_changes() {
        let mut insert = editor_with_text("one");
        type_keys(&mut insert, "A!");
        insert.handle_key(&named("escape"));
        type_keys(&mut insert, ".");
        assert_eq!(insert.buffer.line_text(0).as_deref(), Some("one!!"));

        let mut operator = editor_with_text("one two three");
        type_keys(&mut operator, "dw.");
        assert_eq!(operator.buffer.line_text(0).as_deref(), Some("three"));
    }

    fn execute_ex_keys(editor: &mut Editor, command: &str) {
        type_keys(editor, ":");
        type_keys(editor, command);
        editor.handle_key(&named("enter"));
    }

    fn temporary_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ed-{label}-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ))
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
