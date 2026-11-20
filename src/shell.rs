use std::{
    io::{self, Read},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

use async_channel::Sender;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellRequest {
    pub id: u64,
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

#[derive(Debug)]
pub enum ShellStreamEvent {
    Chunk { request_id: u64, text: String },
    Finished(ShellCompletion),
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

pub fn run_streaming(request: ShellRequest, events: Sender<ShellStreamEvent>) {
    let result = stream_process(&request, &events);
    let _ = events.send_blocking(ShellStreamEvent::Finished(ShellCompletion {
        request,
        result,
    }));
}

fn stream_process(
    request: &ShellRequest,
    events: &Sender<ShellStreamEvent>,
) -> io::Result<ShellResult> {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(&request.command)
        .current_dir(&request.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = Arc::new(Mutex::new(String::new()));
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = spawn_reader(stdout, request.id, output.clone(), events.clone());
    let stderr_reader = spawn_reader(stderr, request.id, output.clone(), events.clone());
    let status = child.wait()?;
    stdout_reader
        .join()
        .map_err(|_| io::Error::other("stdout reader thread panicked"))??;
    stderr_reader
        .join()
        .map_err(|_| io::Error::other("stderr reader thread panicked"))??;
    let text = Arc::try_unwrap(output)
        .map_err(|_| io::Error::other("shell output still shared"))?
        .into_inner()
        .map_err(|_| io::Error::other("shell output lock poisoned"))?;
    Ok(ShellResult {
        output: text,
        exit_code: status.code(),
        success: status.success(),
    })
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    request_id: u64,
    output: Arc<Mutex<String>>,
    events: Sender<ShellStreamEvent>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut bytes = [0; 4096];
        loop {
            let count = reader.read(&mut bytes)?;
            if count == 0 {
                return Ok(());
            }
            let text = String::from_utf8_lossy(&bytes[..count]).into_owned();
            let mut output = output
                .lock()
                .map_err(|_| io::Error::other("shell output lock poisoned"))?;
            output.push_str(&text);
            let _ = events.send_blocking(ShellStreamEvent::Chunk { request_id, text });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_nonzero_exit_status() {
        let completion = run(ShellRequest {
            id: 1,
            command: "printf stdout; printf stderr >&2; exit 7".to_owned(),
            cwd: std::env::current_dir().expect("current directory"),
        });
        let result = completion.result.expect("run shell command");

        assert_eq!(result.output, "stdout\nstderr");
        assert_eq!(result.exit_code, Some(7));
        assert!(!result.success);
    }

    #[test]
    fn emits_streaming_chunks_before_completion() {
        let (sender, receiver) = async_channel::unbounded();
        run_streaming(
            ShellRequest {
                id: 7,
                command: "printf first; printf second".to_owned(),
                cwd: std::env::current_dir().expect("current directory"),
            },
            sender,
        );
        let mut streamed = String::new();
        let mut completion = None;
        while let Ok(event) = receiver.try_recv() {
            match event {
                ShellStreamEvent::Chunk { request_id, text } => {
                    assert_eq!(request_id, 7);
                    streamed.push_str(&text);
                }
                ShellStreamEvent::Finished(finished) => completion = Some(finished),
            }
        }

        assert_eq!(streamed, "firstsecond");
        assert_eq!(
            completion
                .expect("stream completion")
                .result
                .expect("stream result")
                .output,
            "firstsecond"
        );
    }
}
