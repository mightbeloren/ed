use crate::input::{Key, KeyValue};

use super::{Command, Mode, Operator, RegisterName, VisualKind, parser::NormalParser};

#[derive(Debug, Default)]
pub struct VimEngine {
    mode: Mode,
    normal: NormalParser,
    visual_register: RegisterName,
    pending_visual_register: bool,
}

impl VimEngine {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn normal_command(&mut self, key: &Key) -> Option<Command> {
        if self.mode == Mode::Normal && is_control_key(key, "v") {
            return Some(Command::EnterVisual(VisualKind::Block));
        }
        if self.mode == Mode::Normal && is_control_key(key, "r") {
            return Some(self.normal.redo_command());
        }
        if self.mode == Mode::Normal && is_control_key(key, "o") {
            return Some(self.normal.jump_command(false));
        }
        if self.mode == Mode::Normal && is_control_key(key, "i") {
            return Some(self.normal.jump_command(true));
        }
        if !matches!(self.mode, Mode::Normal | Mode::OperatorPending) {
            return None;
        }
        let command = self.normal.parse(key);
        self.mode = if self.normal.is_operator_pending() {
            Mode::OperatorPending
        } else {
            Mode::Normal
        };
        command
    }

    pub fn visual_command(&mut self, key: &Key) -> Option<Command> {
        if !self.is_visual() {
            return None;
        }
        if is_control_key(key, "v") {
            return Some(Command::EnterVisual(VisualKind::Block));
        }
        if matches!(&key.value, KeyValue::Named(name) if name == "escape") {
            return Some(Command::ExitVisual);
        }
        let KeyValue::Character(text) = &key.value else {
            return self.visual_motion(key);
        };
        if key.modifiers.control || key.modifiers.alt || key.modifiers.super_key {
            return None;
        }
        let mut characters = text.chars();
        let character = characters.next()?;
        if characters.next().is_some() {
            return None;
        }

        if self.pending_visual_register {
            self.pending_visual_register = false;
            self.visual_register = match character {
                '_' => RegisterName::BlackHole,
                '"' => RegisterName::Unnamed,
                _ => RegisterName::Unnamed,
            };
            return None;
        }
        if character == '"' {
            self.pending_visual_register = true;
            return None;
        }

        let command = match character {
            'v' => Command::EnterVisual(VisualKind::Character),
            'V' => Command::EnterVisual(VisualKind::Line),
            'o' => Command::SwapVisualEnd,
            'd' | 'x' => self.selection_operator(Operator::Delete),
            'c' => self.selection_operator(Operator::Change),
            'y' => self.selection_operator(Operator::Yank),
            '>' => self.selection_operator(Operator::Indent),
            '<' => self.selection_operator(Operator::Outdent),
            '=' => self.selection_operator(Operator::Reindent),
            '~' => self.selection_operator(Operator::SwapCase),
            'u' => self.selection_operator(Operator::Lowercase),
            'U' => self.selection_operator(Operator::Uppercase),
            'I' if self.mode == Mode::VisualBlock => Command::BeginBlockInsert { append: false },
            'A' if self.mode == Mode::VisualBlock => Command::BeginBlockInsert { append: true },
            _ => return self.visual_motion(key),
        };
        Some(command)
    }

    pub fn enter_insert(&mut self) {
        self.normal.reset();
        self.mode = Mode::Insert;
    }

    pub fn enter_replace(&mut self) {
        self.normal.reset();
        self.mode = Mode::Replace;
    }

    pub fn enter_visual(&mut self, kind: VisualKind) {
        self.normal.reset();
        self.mode = mode_for_visual(kind);
    }

    pub fn leave_transient_mode(&mut self) {
        self.mode = Mode::Normal;
        self.normal.reset();
        self.visual_register = RegisterName::Unnamed;
        self.pending_visual_register = false;
    }

    pub fn leave_insert(&mut self) {
        self.leave_transient_mode();
    }

    pub fn visual_kind(&self) -> Option<VisualKind> {
        match self.mode {
            Mode::VisualCharacter => Some(VisualKind::Character),
            Mode::VisualLine => Some(VisualKind::Line),
            Mode::VisualBlock => Some(VisualKind::Block),
            _ => None,
        }
    }

    fn visual_motion(&mut self, key: &Key) -> Option<Command> {
        match self.normal.parse(key) {
            Some(
                command @ (Command::Move { .. }
                | Command::StartSearch { .. }
                | Command::StartEx
                | Command::RepeatSearch { .. }
                | Command::SearchWord { .. }
                | Command::JumpToMark { .. }
                | Command::ChangeOlder { .. }
                | Command::ChangeNewer { .. }),
            ) => Some(command),
            _ => None,
        }
    }

    fn selection_operator(&mut self, operator: Operator) -> Command {
        let register = self.visual_register;
        self.visual_register = RegisterName::Unnamed;
        Command::OperateSelection { operator, register }
    }

    fn is_visual(&self) -> bool {
        self.visual_kind().is_some()
    }
}

fn mode_for_visual(kind: VisualKind) -> Mode {
    match kind {
        VisualKind::Character => Mode::VisualCharacter,
        VisualKind::Line => Mode::VisualLine,
        VisualKind::Block => Mode::VisualBlock,
    }
}

fn is_control_key(key: &Key, name: &str) -> bool {
    key.modifiers.control && matches!(&key.value, KeyValue::Named(value) if value == name)
}
