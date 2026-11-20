use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use regex::Regex;

use crate::{
    buffer::Buffer,
    shell::{ShellCompletion, ShellResult},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JumpTarget {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug)]
pub struct OutputState {
    request_id: u64,
    command: String,
    cwd: PathBuf,
    status: OutputStatus,
    targets: Vec<Option<JumpTarget>>,
}

#[derive(Clone, Copy, Debug)]
enum OutputStatus {
    Running,
    Finished {
        exit_code: Option<i32>,
        success: bool,
    },
}

impl OutputState {
    pub fn running(request: &crate::shell::ShellRequest) -> (Buffer, Self) {
        (
            Buffer::from_text(&format!("$ {}\n", request.command)),
            Self {
                request_id: request.id,
                command: request.command.clone(),
                cwd: request.cwd.clone(),
                status: OutputStatus::Running,
                targets: vec![None],
            },
        )
    }

    pub fn from_completion(completion: ShellCompletion) -> (Buffer, Self) {
        let request_id = completion.request.id;
        let command = completion.request.command;
        let cwd = completion.request.cwd;
        let ShellResult {
            mut output,
            exit_code,
            success,
        } = completion.result.unwrap_or_else(|error| ShellResult {
            output: format!("Could not run shell command: {error}"),
            exit_code: None,
            success: false,
        });
        if output.is_empty() {
            output = "(no output)".to_owned();
        }
        let text = format!("$ {command}\n{output}");
        let targets = text
            .lines()
            .map(|line| parse_jump_target(line, &cwd))
            .collect();
        (
            Buffer::from_text(&text),
            Self {
                request_id,
                command,
                cwd,
                status: OutputStatus::Finished { exit_code, success },
                targets,
            },
        )
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn target(&self, line: usize) -> Option<&JumpTarget> {
        self.targets.get(line)?.as_ref()
    }

    pub fn is_running_request(&self, request_id: u64) -> bool {
        self.request_id == request_id && matches!(self.status, OutputStatus::Running)
    }

    pub fn display_name(&self) -> String {
        match self.status {
            OutputStatus::Running => format!("Output: {} [running]", self.command),
            OutputStatus::Finished { exit_code, success } => {
                let status =
                    exit_code.map_or_else(|| "signal".to_owned(), |code| format!("exit {code}"));
                let outcome = if success { "ok" } else { "failed" };
                format!("Output: {} [{status}, {outcome}]", self.command)
            }
        }
    }
}

pub fn parse_jump_target(line: &str, cwd: &Path) -> Option<JumpTarget> {
    static LOCATION: OnceLock<Regex> = OnceLock::new();
    let location = LOCATION.get_or_init(|| {
        Regex::new(
            r#"(?P<path>(?:/|\./|\.\./)?[^\s:'\"]+):(?P<line>[1-9]\d*)(?::(?P<column>[1-9]\d*))?"#,
        )
        .expect("valid output location regex")
    });

    for captures in location.captures_iter(line) {
        let path = captures.name("path")?.as_str();
        let Some(path) = existing_file(path, cwd) else {
            continue;
        };
        let line = captures.name("line")?.as_str().parse::<usize>().ok()?;
        let column = captures
            .name("column")
            .and_then(|column| column.as_str().parse::<usize>().ok())
            .unwrap_or(1);
        return Some(JumpTarget {
            path,
            line: line - 1,
            column: column - 1,
        });
    }

    line.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|character: char| {
            matches!(
                character,
                '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';'
            )
        });
        existing_file(token, cwd).map(|path| JumpTarget {
            path,
            line: 0,
            column: 0,
        })
    })
}

fn existing_file(path: &str, cwd: &Path) -> Option<PathBuf> {
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    path.is_file().then(|| path.canonicalize().unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::*;

    #[test]
    fn parses_relative_path_line_and_column_targets() {
        let directory = std::env::temp_dir().join(format!(
            "ed-output-target-test-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("create fixture directory");
        let path = directory.join("main.rs");
        fs::write(&path, "one\ntwo\nthree").expect("write fixture");

        assert_eq!(
            parse_jump_target("error: main.rs:3:2: failed", &directory),
            Some(JumpTarget {
                path: path.canonicalize().expect("canonical fixture"),
                line: 2,
                column: 1
            })
        );

        fs::remove_dir_all(directory).expect("remove fixture directory");
    }

    #[test]
    fn parses_plain_existing_paths() {
        let directory = std::env::temp_dir().join(format!(
            "ed-output-plain-target-test-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("create fixture directory");
        let path = directory.join("notes.txt");
        fs::write(&path, "notes").expect("write fixture");

        assert_eq!(
            parse_jump_target("generated notes.txt", &directory),
            Some(JumpTarget {
                path: path.canonicalize().expect("canonical fixture"),
                line: 0,
                column: 0
            })
        );

        fs::remove_dir_all(directory).expect("remove fixture directory");
    }
}
