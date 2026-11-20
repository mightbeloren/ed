#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Move { motion: Motion, count: u32 },
    GoToLine { line: Option<usize> },
    EnterInsert(InsertPlacement),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Motion {
    Left,
    Down,
    Up,
    Right,
    LineStart,
    FirstNonBlank,
    LineEnd,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InsertPlacement {
    BeforeCursor,
    AfterCursor,
    FirstNonBlank,
    EndOfLine,
    OpenBelow,
    OpenAbove,
}
