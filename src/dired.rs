use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use crate::{
    buffer::Buffer,
    input::{Key, KeyValue},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiredEntry {
    pub path: PathBuf,
    pub is_directory: bool,
}

#[derive(Debug)]
pub struct DiredState {
    directory: PathBuf,
    entries: Vec<DiredEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiredCommand {
    Open,
    Parent,
}

impl DiredState {
    pub fn load(path: impl AsRef<Path>) -> io::Result<(Buffer, Self)> {
        let directory = fs::canonicalize(path)?;
        if !directory.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", directory.display()),
            ));
        }

        let parent = directory
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory.clone());
        let mut rows = vec![(
            DiredEntry {
                path: parent,
                is_directory: true,
            },
            "drwxr-xr-x          -          - ../".to_owned(),
        )];

        let mut children = Vec::new();
        for entry in fs::read_dir(&directory)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let is_directory = path.is_dir();
            let name = entry.file_name().to_string_lossy().into_owned();
            let display_name = escaped_name(&name);
            let suffix = if is_directory { "/" } else { "" };
            let modified = metadata.mtime();
            let row = format!(
                "{} {:>10} {:>10} {display_name}{suffix}",
                mode_string(&metadata),
                metadata.len(),
                modified
            );
            children.push((DiredEntry { path, is_directory }, row, name));
        }
        children.sort_by(|left, right| {
            right
                .0
                .is_directory
                .cmp(&left.0.is_directory)
                .then_with(|| left.2.to_lowercase().cmp(&right.2.to_lowercase()))
                .then_with(|| left.2.cmp(&right.2))
        });
        rows.extend(children.into_iter().map(|(entry, row, _name)| (entry, row)));

        let text = rows
            .iter()
            .map(|(_, row)| row.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let entries = rows.into_iter().map(|(entry, _)| entry).collect();
        Ok((Buffer::from_text(&text), Self { directory, entries }))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn entry(&self, line: usize) -> Option<&DiredEntry> {
        self.entries.get(line)
    }

    pub fn line_for_path(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|entry| entry.path == path)
    }
}

pub fn command_for_key(key: &Key) -> Option<DiredCommand> {
    if key.modifiers.control || key.modifiers.alt || key.modifiers.super_key {
        return None;
    }
    match &key.value {
        KeyValue::Named(name) if name == "enter" => Some(DiredCommand::Open),
        KeyValue::Named(name) if name == "backspace" => Some(DiredCommand::Parent),
        _ => None,
    }
}

fn escaped_name(name: &str) -> String {
    let mut escaped = String::with_capacity(name.len());
    for character in name.chars() {
        match character {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn mode_string(metadata: &fs::Metadata) -> String {
    let kind = if metadata.file_type().is_dir() {
        'd'
    } else if metadata.file_type().is_symlink() {
        'l'
    } else {
        '-'
    };
    let mode = metadata.permissions().mode();
    let mut result = String::with_capacity(10);
    result.push(kind);
    for (read, write, execute) in [
        (0o400, 0o200, 0o100),
        (0o040, 0o020, 0o010),
        (0o004, 0o002, 0o001),
    ] {
        result.push(if mode & read != 0 { 'r' } else { '-' });
        result.push(if mode & write != 0 { 'w' } else { '-' });
        result.push(if mode & execute != 0 { 'x' } else { '-' });
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::*;
    use crate::input::Modifiers;

    #[test]
    fn loads_parent_directories_before_sorted_files() {
        let directory = std::env::temp_dir().join(format!(
            "ed-dired-test-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("create fixture directory");
        fs::create_dir(directory.join("folder")).expect("create child directory");
        fs::write(directory.join("zeta.txt"), "z").expect("write zeta fixture");
        fs::write(directory.join("alpha.txt"), "a").expect("write alpha fixture");

        let (buffer, dired) = DiredState::load(&directory).expect("load Dired");

        assert!(dired.entry(0).expect("parent entry").is_directory);
        assert_eq!(
            dired.entry(1).expect("directory entry").path,
            directory.join("folder")
        );
        assert_eq!(
            dired.entry(2).expect("first file").path,
            directory.join("alpha.txt")
        );
        assert!(
            buffer
                .line_text(1)
                .expect("directory row")
                .ends_with("folder/")
        );
        assert!(!buffer.is_modified());

        fs::remove_dir_all(directory).expect("remove fixture directory");
    }

    #[test]
    fn escapes_newlines_without_breaking_entry_line_mapping() {
        let directory = std::env::temp_dir().join(format!(
            "ed-dired-newline-test-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("create fixture directory");
        let path = directory.join("line\nbreak.txt");
        fs::write(&path, "contents").expect("write newline fixture");

        let (buffer, dired) = DiredState::load(&directory).expect("load Dired");

        assert_eq!(buffer.len_lines(), 2);
        assert_eq!(dired.entry(1).expect("newline entry").path, path);
        assert!(
            buffer
                .line_text(1)
                .expect("escaped row")
                .ends_with("line\\nbreak.txt")
        );

        fs::remove_dir_all(directory).expect("remove fixture directory");
    }

    #[test]
    fn maps_only_special_named_keys() {
        let enter = Key {
            value: KeyValue::Named("enter".to_owned()),
            modifiers: Modifiers::default(),
        };
        assert_eq!(command_for_key(&enter), Some(DiredCommand::Open));
    }
}
