use super::CharOffset;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EditTransaction {
    pub edits: Vec<TextEdit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextEdit {
    pub start: CharOffset,
    pub deleted_text: String,
    pub inserted_text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UndoNode {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub preferred_child: Option<usize>,
    pub transaction: Option<EditTransaction>,
}
