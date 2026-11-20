use crate::input::Key;

use super::{Command, Mode, parser::NormalParser};

#[derive(Debug, Default)]
pub struct VimEngine {
    mode: Mode,
    normal: NormalParser,
}

impl VimEngine {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn normal_command(&mut self, key: &Key) -> Option<Command> {
        (self.mode == Mode::Normal)
            .then(|| self.normal.parse(key))
            .flatten()
    }

    pub fn enter_insert(&mut self) {
        self.normal.reset();
        self.mode = Mode::Insert;
    }

    pub fn leave_insert(&mut self) {
        self.mode = Mode::Normal;
        self.normal.reset();
    }
}
