use std::{
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process,
};

use ropey::Rope;

use super::{BufferPosition, CharOffset, EditTransaction, InvalidPosition, TextEdit, UndoNode};

#[derive(Debug)]
pub enum BufferError {
    Io(io::Error),
    NoPath,
}

impl fmt::Display for BufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::NoPath => formatter.write_str("buffer has no file path"),
        }
    }
}

impl std::error::Error for BufferError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::NoPath => None,
        }
    }
}

impl From<io::Error> for BufferError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct Buffer {
    path: Option<PathBuf>,
    text: Rope,
    revision: u64,
    saved_undo: usize,
    untracked_modified: bool,
    active_transaction: Option<EditTransaction>,
    completed_transactions: Vec<EditTransaction>,
    undo_nodes: Vec<UndoNode>,
    current_undo: usize,
}

impl Buffer {
    pub fn scratch() -> Self {
        Self::from_rope(None, Rope::new())
    }

    pub fn from_text(text: &str) -> Self {
        Self::from_rope(None, Rope::from_str(text))
    }

    fn from_rope(path: Option<PathBuf>, text: Rope) -> Self {
        Self {
            path,
            text,
            revision: 0,
            saved_undo: 0,
            untracked_modified: false,
            active_transaction: None,
            completed_transactions: Vec::new(),
            undo_nodes: vec![UndoNode::default()],
            current_undo: 0,
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, BufferError> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let text = Rope::from_reader(BufReader::new(file))?;

        Ok(Self::from_rope(Some(path), text))
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[No Name]".to_owned())
    }

    pub fn is_modified(&self) -> bool {
        self.untracked_modified
            || self.current_undo != self.saved_undo
            || self
                .active_transaction
                .as_ref()
                .is_some_and(|transaction| !transaction.edits.is_empty())
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    pub fn len_lines(&self) -> usize {
        self.text.len_lines()
    }

    pub fn logical_len_lines(&self) -> usize {
        let lines = self.text.len_lines();
        if lines > 1 && self.text.line(lines - 1).len_chars() == 0 {
            lines - 1
        } else {
            lines
        }
    }

    pub fn line_start_offset(&self, line: usize) -> Option<CharOffset> {
        (line < self.text.len_lines()).then(|| CharOffset(self.text.line_to_char(line)))
    }

    pub fn line_len_chars(&self, line: usize) -> Option<usize> {
        let line = self.text.get_line(line)?;
        let mut length = line.len_chars();
        if length > 0 && line.char(length - 1) == '\n' {
            length -= 1;
            if length > 0 && line.char(length - 1) == '\r' {
                length -= 1;
            }
        }
        Some(length)
    }

    pub fn line_text(&self, line_index: usize) -> Option<String> {
        let line = self.text.get_line(line_index)?;
        let length = self.line_len_chars(line_index)?;
        Some(line.slice(..length).to_string())
    }

    pub fn position_to_offset(
        &self,
        position: BufferPosition,
    ) -> Result<CharOffset, InvalidPosition> {
        let Some(line_length) = self.line_len_chars(position.line) else {
            return Err(InvalidPosition { position });
        };
        if position.character > line_length {
            return Err(InvalidPosition { position });
        }

        Ok(CharOffset(
            self.text.line_to_char(position.line) + position.character,
        ))
    }

    pub fn offset_to_position(
        &self,
        offset: CharOffset,
    ) -> Result<BufferPosition, InvalidPosition> {
        if offset.0 > self.text.len_chars() {
            return Err(InvalidPosition {
                position: BufferPosition {
                    line: self.text.len_lines(),
                    character: 0,
                },
            });
        }

        let line = self.text.char_to_line(offset.0);
        Ok(BufferPosition {
            line,
            character: offset.0 - self.text.line_to_char(line),
        })
    }

    pub fn char_at(&self, offset: CharOffset) -> Option<char> {
        self.text.get_char(offset.0)
    }

    pub fn text(&self, start: CharOffset, end: CharOffset) -> Option<String> {
        (start <= end && end.0 <= self.text.len_chars())
            .then(|| self.text.slice(start.0..end.0).to_string())
    }

    pub fn begin_transaction(&mut self) {
        debug_assert!(self.active_transaction.is_none());
        self.active_transaction = Some(EditTransaction::default());
    }

    pub fn commit_transaction(&mut self) {
        if let Some(transaction) = self.active_transaction.take()
            && !transaction.edits.is_empty()
        {
            let parent = self.current_undo;
            let node = self.undo_nodes.len();
            self.undo_nodes.push(UndoNode {
                parent: Some(parent),
                transaction: Some(transaction.clone()),
                ..UndoNode::default()
            });
            self.undo_nodes[parent].children.push(node);
            self.undo_nodes[parent].preferred_child = Some(node);
            self.current_undo = node;
            self.completed_transactions.push(transaction);
        }
    }

    pub fn completed_transaction_count(&self) -> usize {
        self.completed_transactions.len()
    }

    pub fn last_completed_transaction_start(&self) -> Option<CharOffset> {
        self.completed_transactions
            .last()?
            .edits
            .first()
            .map(|edit| edit.start)
    }

    pub fn undo(&mut self) -> Option<CharOffset> {
        let node = self.current_undo;
        let parent = self.undo_nodes.get(node)?.parent?;
        let transaction = self.undo_nodes[node].transaction.clone()?;
        for edit in transaction.edits.iter().rev() {
            let inserted = edit.inserted_text.chars().count();
            self.text.remove(edit.start.0..edit.start.0 + inserted);
            self.text.insert(edit.start.0, &edit.deleted_text);
        }
        self.undo_nodes[parent].preferred_child = Some(node);
        self.current_undo = parent;
        self.revision = self.revision.wrapping_add(1);
        transaction.edits.first().map(|edit| edit.start)
    }

    pub fn redo(&mut self) -> Option<CharOffset> {
        let node = self.undo_nodes.get(self.current_undo)?;
        let child = node
            .preferred_child
            .or_else(|| node.children.last().copied())?;
        let transaction = self.undo_nodes[child].transaction.clone()?;
        for edit in &transaction.edits {
            let deleted = edit.deleted_text.chars().count();
            self.text.remove(edit.start.0..edit.start.0 + deleted);
            self.text.insert(edit.start.0, &edit.inserted_text);
        }
        self.current_undo = child;
        self.revision = self.revision.wrapping_add(1);
        transaction.edits.first().map(|edit| edit.start)
    }

    pub fn undo_branch_count(&self) -> usize {
        self.undo_nodes
            .iter()
            .map(|node| node.children.len())
            .max()
            .unwrap_or(0)
    }

    pub fn insert(&mut self, offset: CharOffset, text: &str) -> Result<(), InvalidPosition> {
        if offset.0 > self.text.len_chars() {
            return Err(InvalidPosition {
                position: BufferPosition {
                    line: self.text.len_lines(),
                    character: 0,
                },
            });
        }
        if text.is_empty() {
            return Ok(());
        }

        self.text.insert(offset.0, text);
        self.record_edit(TextEdit {
            start: offset,
            deleted_text: String::new(),
            inserted_text: text.to_owned(),
        });
        Ok(())
    }

    pub fn remove(&mut self, start: CharOffset, end: CharOffset) -> Result<(), InvalidPosition> {
        if start > end || end.0 > self.text.len_chars() {
            return Err(InvalidPosition {
                position: BufferPosition {
                    line: self.text.len_lines(),
                    character: 0,
                },
            });
        }
        if start == end {
            return Ok(());
        }

        let deleted_text = self.text.slice(start.0..end.0).to_string();
        self.text.remove(start.0..end.0);
        self.record_edit(TextEdit {
            start,
            deleted_text,
            inserted_text: String::new(),
        });
        Ok(())
    }

    fn record_edit(&mut self, edit: TextEdit) {
        self.revision = self.revision.wrapping_add(1);
        if let Some(transaction) = &mut self.active_transaction {
            transaction.edits.push(edit);
        } else {
            self.untracked_modified = true;
        }
    }

    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<(), BufferError> {
        let path = path.as_ref();
        let temporary_path = temporary_path_for(path, self.revision);
        let result = self.write_atomic(path, &temporary_path);
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), BufferError> {
        let previous_path = self.path.replace(path.as_ref().to_path_buf());
        if let Err(error) = self.save() {
            self.path = previous_path;
            return Err(error);
        }
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), BufferError> {
        let path = self.path.clone().ok_or(BufferError::NoPath)?;
        let temporary_path = temporary_path_for(&path, self.revision);
        let result = self.write_atomic(&path, &temporary_path);
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result?;
        self.saved_undo = self.current_undo;
        self.untracked_modified = false;
        Ok(())
    }

    fn write_atomic(&self, path: &Path, temporary_path: &Path) -> Result<(), BufferError> {
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(temporary_path)?;
        let mut writer = BufWriter::new(file);
        self.text.write_to(&mut writer)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;

        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(temporary_path, metadata.permissions())?;
        }

        fs::rename(temporary_path, path)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}

fn temporary_path_for(path: &Path, revision: u64) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(
        path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("buffer")),
    );
    name.push(format!(".ed-{}-{revision}.tmp", process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unicode_positions_to_character_offsets() {
        let mut buffer = Buffer::scratch();
        buffer
            .insert(CharOffset(0), "alpha\nβeta 🦀\n")
            .expect("valid insertion");

        let position = BufferPosition {
            line: 1,
            character: 5,
        };
        let offset = buffer.position_to_offset(position).expect("valid position");

        assert_eq!(offset, CharOffset(11));
        assert_eq!(buffer.offset_to_position(offset), Ok(position));
    }

    #[test]
    fn excludes_line_endings_from_editable_line_length() {
        let mut buffer = Buffer::scratch();
        buffer
            .insert(CharOffset(0), "one\r\ntwo\n")
            .expect("valid insertion");

        assert_eq!(buffer.line_len_chars(0), Some(3));
        assert_eq!(buffer.line_len_chars(1), Some(3));
        assert_eq!(buffer.line_text(0).as_deref(), Some("one"));
    }

    #[test]
    fn tracks_modified_state_across_edits() {
        let mut buffer = Buffer::scratch();
        assert!(!buffer.is_modified());

        buffer
            .insert(CharOffset(0), "changed")
            .expect("valid insertion");
        assert!(buffer.is_modified());

        buffer
            .remove(CharOffset(0), CharOffset(7))
            .expect("valid removal");
        assert!(buffer.is_modified());
    }

    #[test]
    fn empty_transactions_are_not_recorded() {
        let mut buffer = Buffer::scratch();

        buffer.begin_transaction();
        buffer.commit_transaction();

        assert_eq!(buffer.completed_transaction_count(), 0);
    }

    #[test]
    fn atomically_saves_an_open_file() {
        let directory = std::env::temp_dir().join(format!(
            "ed-buffer-test-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("create test directory");
        let path = directory.join("document.txt");
        fs::write(&path, "before").expect("write fixture");

        let mut buffer = Buffer::open(&path).expect("open fixture");
        buffer
            .insert(CharOffset(6), " after")
            .expect("valid insertion");
        buffer.save().expect("save buffer");

        assert_eq!(
            fs::read_to_string(&path).expect("read saved file"),
            "before after"
        );
        assert!(!buffer.is_modified());
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn undoing_to_the_saved_history_node_clears_modified_state() {
        let directory = std::env::temp_dir().join(format!(
            "ed-buffer-undo-save-test-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("create test directory");
        let path = directory.join("document.txt");
        fs::write(&path, "before").expect("write fixture");

        let mut buffer = Buffer::open(&path).expect("open fixture");
        buffer.begin_transaction();
        buffer
            .insert(CharOffset(6), " saved")
            .expect("valid insertion");
        buffer.commit_transaction();
        buffer.save().expect("save buffer");
        assert!(!buffer.is_modified());

        buffer.begin_transaction();
        buffer
            .insert(CharOffset(12), " changed")
            .expect("valid insertion");
        buffer.commit_transaction();
        assert!(buffer.is_modified());

        buffer.undo().expect("undo latest transaction");
        assert_eq!(buffer.line_text(0).as_deref(), Some("before saved"));
        assert!(!buffer.is_modified());

        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
