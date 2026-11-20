use crate::vim::Direction;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MinibufferKind {
    Search(Direction),
    Ex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Minibuffer {
    pub kind: MinibufferKind,
    pub text: String,
}

impl Minibuffer {
    pub fn search(direction: Direction) -> Self {
        Self {
            kind: MinibufferKind::Search(direction),
            text: String::new(),
        }
    }

    pub fn ex(initial: impl Into<String>) -> Self {
        Self {
            kind: MinibufferKind::Ex,
            text: initial.into(),
        }
    }

    pub fn display(&self) -> String {
        let prefix = match self.kind {
            MinibufferKind::Search(Direction::Forward) => '/',
            MinibufferKind::Search(Direction::Backward) => '?',
            MinibufferKind::Ex => ':',
        };
        format!("{prefix}{}", self.text)
    }
}
