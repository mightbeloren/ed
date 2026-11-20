#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Replace,
    OperatorPending,
    VisualCharacter,
    VisualLine,
    VisualBlock,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Replace => "REPLACE",
            Self::OperatorPending => "OPERATOR",
            Self::VisualCharacter => "VISUAL",
            Self::VisualLine => "VISUAL LINE",
            Self::VisualBlock => "VISUAL BLOCK",
        }
    }
}
