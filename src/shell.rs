use std::{io, path::PathBuf, process::Command};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellRequest {
    pub command: String,
    pub cwd: PathBuf,
}

#[derive(Debug)]
pub struct ShellCompletion {
    pub request: ShellRequest,
    pub result: io::Result<ShellResult>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellResult {
    pub output: String,
    pub exit_code: Option<i32>,
    pub success: bool,
}

pub fn run(request: ShellRequest) -> ShellCompletion {
    let result = Command::new("/bin/sh")
        .arg("-c")
        .arg(&request.command)
        .current_dir(&request.cwd)
        .output()
        .map(|output| {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&stderr);
            }
            ShellResult {
                output: text,
                exit_code: output.status.code(),
                success: output.status.success(),
            }
        });
    ShellCompletion { request, result }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_nonzero_exit_status() {
        let completion = run(ShellRequest {
            command: "printf stdout; printf stderr >&2; exit 7".to_owned(),
            cwd: std::env::current_dir().expect("current directory"),
        });
        let result = completion.result.expect("run shell command");

        assert_eq!(result.output, "stdout\nstderr");
        assert_eq!(result.exit_code, Some(7));
        assert!(!result.success);
    }
}
