use std::{fs, path::Path};

use crate::vim::Direction;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellPathCompletion {
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

    pub fn complete_shell_path(&mut self, cwd: &Path) -> ShellPathCompletion {
        if self.kind != MinibufferKind::Ex {
            return ShellPathCompletion::None;
        }
        let Some((start, prefix)) = shell_path_prefix(&self.text) else {
            return ShellPathCompletion::None;
        };
        let (directory_prefix, name_prefix) = prefix.rfind('/').map_or(("", prefix), |index| {
            (&prefix[..=index], &prefix[index + 1..])
        });
        let directory = cwd.join(directory_prefix);
        let Ok(entries) = fs::read_dir(directory) else {
            return ShellPathCompletion::None;
        };
        let mut candidates = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with(name_prefix)
                    || (!name_prefix.starts_with('.') && name.starts_with('.'))
                {
                    return None;
                }
                let suffix = if entry.file_type().ok()?.is_dir() {
                    "/"
                } else {
                    ""
                };
                Some(format!("{directory_prefix}{name}{suffix}"))
            })
            .collect::<Vec<_>>();
        candidates.sort();
        if candidates.is_empty() {
            return ShellPathCompletion::None;
        }

        let exact = candidates
            .iter()
            .find(|candidate| candidate.as_str() == prefix);
        let replacement = exact.cloned().unwrap_or_else(|| common_prefix(&candidates));
        if replacement.len() > prefix.len() {
            self.text.replace_range(start.., &replacement);
        }
        if exact.is_some() || candidates.len() == 1 {
            if !self.text.ends_with('/') {
                self.text.push(' ');
            }
            ShellPathCompletion::Completed
        } else {
            ShellPathCompletion::Candidates(candidates)
        }
    }
}

fn shell_path_prefix(text: &str) -> Option<(usize, &str)> {
    let body_start = if let Some(rest) = text.strip_prefix('!') {
        1 + (rest.len() - rest.trim_start().len())
    } else {
        let rest = text.strip_prefix("run")?;
        if !rest.chars().next().is_some_and(char::is_whitespace) {
            return None;
        }
        3 + (rest.len() - rest.trim_start().len())
    };
    let body = &text[body_start..];
    let argument_separator = body.rfind(char::is_whitespace)?;
    let start = body_start + argument_separator + 1;
    let prefix = &text[start..];
    (!prefix.chars().any(char::is_whitespace)).then_some((start, prefix))
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
    fn completes_file_arguments_for_bang_and_run_prompts() {
        let directory = std::env::temp_dir().join(format!(
            "ed-shell-path-completion-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        let source = directory.join("src");
        fs::create_dir_all(&source).expect("create completion directory");
        fs::write(source.join("file.txt"), "contents").expect("write file fixture");
        fs::write(source.join("filter.rs"), "contents").expect("write second fixture");

        let mut bang = Minibuffer::ex("!cat src/file");
        assert_eq!(
            bang.complete_shell_path(&directory),
            ShellPathCompletion::Completed
        );
        assert_eq!(bang.text, "!cat src/file.txt ");

        let mut run = Minibuffer::ex("run cat src/fi");
        assert_eq!(
            run.complete_shell_path(&directory),
            ShellPathCompletion::Candidates(vec![
                "src/file.txt".to_owned(),
                "src/filter.rs".to_owned()
            ])
        );
        assert_eq!(run.text, "run cat src/fil");

        let mut command = Minibuffer::ex("!ca");
        assert_eq!(
            command.complete_shell_path(&directory),
            ShellPathCompletion::None
        );
        assert_eq!(command.text, "!ca");

        fs::remove_dir_all(directory).expect("remove completion directory");
    }
}
