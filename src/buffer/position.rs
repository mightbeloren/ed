use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct CharOffset(pub usize);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BufferPosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPosition {
    pub position: BufferPosition,
}

impl fmt::Display for InvalidPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid buffer position {}:{}",
            self.position.line + 1,
            self.position.character + 1
        )
    }
}

impl std::error::Error for InvalidPosition {}
