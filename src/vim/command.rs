#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Move {
        motion: Motion,
        count: u32,
    },
    EnterInsert(InsertPlacement),
    Operate(OperatorCommand),
    EnterVisual(VisualKind),
    ExitVisual,
    SwapVisualEnd,
    OperateSelection {
        operator: Operator,
        register: RegisterName,
    },
    DeleteCharacters {
        count: u32,
        backward: bool,
    },
    EnterReplace,
    ReplaceCharacter {
        character: char,
        count: u32,
    },
    BeginBlockInsert {
        append: bool,
    },
    Paste {
        after: bool,
        count: u32,
        register: RegisterName,
    },
    Undo {
        count: u32,
    },
    Redo {
        count: u32,
    },
    BeginMacro {
        register: char,
    },
    PlayMacro {
        register: Option<char>,
        count: u32,
    },
    RepeatChange {
        count: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Motion {
    Left,
    Down,
    Up,
    Right,
    WordForward { big_word: bool },
    WordBackward { big_word: bool },
    WordEnd { big_word: bool },
    LineStart,
    FirstNonBlank,
    LineEnd,
    GoToLine { line: Option<usize> },
    FindCharacter(FindMotion),
    RepeatFind { reverse: bool },
    MatchingDelimiter,
    PageUp,
    PageDown,
    Underscore,
    NextLineFirstNonBlank,
    PreviousLineFirstNonBlank,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindMotion {
    pub character: char,
    pub direction: Direction,
    pub till: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualKind {
    Character,
    Line,
    Block,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperatorCommand {
    pub operator: Operator,
    pub target: Target,
    pub count: u32,
    pub register: RegisterName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Motion(Motion),
    TextObject(TextObject),
    CurrentLine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextObject {
    pub scope: TextObjectScope,
    pub kind: TextObjectKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextObjectScope {
    Inner,
    Around,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextObjectKind {
    Word { big_word: bool },
    Quotes(char),
    Delimited { open: char, close: char },
    Paragraph,
    Tag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Reindent,
    SwapCase,
    Lowercase,
    Uppercase,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RegisterName {
    #[default]
    Unnamed,
    BlackHole,
    Named {
        name: char,
        append: bool,
    },
    Numbered(u8),
    Yank,
    SmallDelete,
    Clipboard,
    Primary,
}
