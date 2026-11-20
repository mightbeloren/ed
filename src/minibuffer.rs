use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use crate::vim::Direction;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellCommandCompletion {
    None,
    Completed,
    Candidates(Vec<String>),
}

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

    pub fn complete_shell_command(&mut self) -> ShellCommandCompletion {
        let Some(path) = std::env::var_os("PATH") else {
            return ShellCommandCompletion::None;
        };
        let directories: Vec<PathBuf> = std::env::split_paths(&path).collect();
        self.complete_shell_command_from_paths(&directories)
    }

    fn complete_shell_command_from_paths(
        &mut self,
        directories: &[PathBuf],
    ) -> ShellCommandCompletion {
        if self.kind != MinibufferKind::Ex {
            return ShellCommandCompletion::None;
        }
        let Some((start, prefix)) = shell_command_prefix(&self.text) else {
            return ShellCommandCompletion::None;
        };
        let candidates = executable_candidates(prefix, directories);
        if candidates.is_empty() {
            return ShellCommandCompletion::None;
        }
        if candidates.iter().any(|candidate| candidate == prefix) {
            self.text.push(' ');
            return ShellCommandCompletion::Completed;
        }
        let common = common_prefix(&candidates);
        if common.len() > prefix.len() {
            self.text.replace_range(start.., &common);
        }
        if candidates.len() == 1 {
            self.text.push(' ');
            ShellCommandCompletion::Completed
        } else {
            ShellCommandCompletion::Candidates(candidates)
        }
    }
}

fn shell_command_prefix(text: &str) -> Option<(usize, &str)> {
    let start = if let Some(rest) = text.strip_prefix('!') {
        1 + (rest.len() - rest.trim_start().len())
    } else {
        let rest = text.strip_prefix("run")?;
        if !rest.chars().next().is_some_and(char::is_whitespace) {
            return None;
        }
        3 + (rest.len() - rest.trim_start().len())
    };
    let prefix = &text[start..];
    (!prefix.is_empty() && !prefix.chars().any(char::is_whitespace)).then_some((start, prefix))
}

fn executable_candidates(prefix: &str, directories: &[PathBuf]) -> Vec<String> {
    let mut candidates = BTreeSet::new();
    for directory in directories {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(prefix) || !is_executable(&entry.path()) {
                continue;
            }
            candidates.insert(name);
        }
    }
    candidates.into_iter().collect()
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn common_prefix(values: &[String]) -> String {
    let Some(first) = values.first() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for value in &values[1..] {
        let matching = prefix
            .iter()
            .copied()
            .zip(value.chars())
            .take_while(|(left, right)| left == right)
            .count();
        prefix.truncate(matching);
    }
    prefix.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::{process, time::SystemTime};

    use super::*;

    #[test]
    fn completes_shell_commands_for_bang_and_run_prompts() {
        let directory = std::env::temp_dir().join(format!(
            "ed-shell-completion-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("create completion directory");
        for name in ["cargo", "cat"] {
            let path = directory.join(name);
            fs::write(&path, "#!/bin/sh\n").expect("write executable fixture");
            let mut permissions = fs::metadata(&path).expect("fixture metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("mark fixture executable");
        }
        fs::write(directory.join("calendar"), "not executable").expect("write ignored fixture");

        let mut bang = Minibuffer::ex("!car");
        assert_eq!(
            bang.complete_shell_command_from_paths(std::slice::from_ref(&directory)),
            ShellCommandCompletion::Completed
        );
        assert_eq!(bang.text, "!cargo ");

        let mut run = Minibuffer::ex("run ca");
        assert_eq!(
            run.complete_shell_command_from_paths(std::slice::from_ref(&directory)),
            ShellCommandCompletion::Candidates(vec!["cargo".to_owned(), "cat".to_owned()])
        );
        assert_eq!(run.text, "run ca");

        fs::remove_dir_all(directory).expect("remove completion directory");
    }
}
