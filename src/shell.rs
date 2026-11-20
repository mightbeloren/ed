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

pub fn run_background(request: ShellRequest) -> ShellCompletion {
    let result = Command::new("/bin/sh")
        .arg("-c")
        .arg(&request.command)
        .current_dir(&request.cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| ShellResult {
            output: String::new(),
            exit_code: status.code(),
            success: status.success(),
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
    let mut child = Command::new("/usr/bin/script")
        .arg("-qefc")
        .arg(&request.command)
        .arg("/dev/null")
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
        let mut filter = TerminalFilter::default();
        loop {
            let count = reader.read(&mut bytes)?;
            if count == 0 {
                let text = filter.finish();
                emit_chunk(request_id, text, &output, &events)?;
                return Ok(());
            }
            let text = filter.push(&bytes[..count]);
            emit_chunk(request_id, text, &output, &events)?;
        }
    })
}

fn emit_chunk(
    request_id: u64,
    text: String,
    output: &Mutex<String>,
    events: &Sender<ShellStreamEvent>,
) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("shell output lock poisoned"))?;
    output.push_str(&text);
    let _ = events.send_blocking(ShellStreamEvent::Chunk { request_id, text });
    Ok(())
}

#[derive(Default)]
struct TerminalFilter {
    escape: EscapeState,
    pending_carriage_return: bool,
}

#[derive(Clone, Copy, Default)]
enum EscapeState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

impl TerminalFilter {
    fn push(&mut self, bytes: &[u8]) -> String {
        let mut visible = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            match self.escape {
                EscapeState::Ground => {
                    if self.pending_carriage_return {
                        visible.push(b'\n');
                        self.pending_carriage_return = false;
                        if byte == b'\n' {
                            continue;
                        }
                    }
                    match byte {
                        b'\x1b' => self.escape = EscapeState::Escape,
                        b'\r' => self.pending_carriage_return = true,
                        b'\n' | b'\t' | b' '..=b'~' | 0x80..=0xff => visible.push(byte),
                        _ => {}
                    }
                }
                EscapeState::Escape => {
                    self.escape = match byte {
                        b'[' => EscapeState::Csi,
                        b']' => EscapeState::Osc,
                        _ => EscapeState::Ground,
                    }
                }
                EscapeState::Csi => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.escape = EscapeState::Ground;
                    }
                }
                EscapeState::Osc => match byte {
                    b'\x07' => self.escape = EscapeState::Ground,
                    b'\x1b' => self.escape = EscapeState::OscEscape,
                    _ => {}
                },
                EscapeState::OscEscape => {
                    self.escape = if byte == b'\\' {
                        EscapeState::Ground
                    } else {
                        EscapeState::Osc
                    };
                }
            }
        }
        String::from_utf8_lossy(&visible).into_owned()
    }

    fn finish(&mut self) -> String {
        if self.pending_carriage_return {
            self.pending_carriage_return = false;
            "\n".to_owned()
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_commands_discard_output_and_report_status() {
        let completion = run_background(ShellRequest {
            id: 2,
            command: "printf ignored; exit 4".to_owned(),
            cwd: std::env::current_dir().expect("current directory"),
        });
        let result = completion.result.expect("run background command");

        assert_eq!(result.output, "");
        assert_eq!(result.exit_code, Some(4));
        assert!(!result.success);
    }

    #[test]
    fn terminal_filter_removes_escapes_and_normalizes_carriage_returns() {
        let mut filter = TerminalFilter::default();
        let text = filter.push(b"\x1b[31mred\x1b[0m\rnext\r\n");

        assert_eq!(text, "red\nnext\n");
    }

    #[test]
    fn streaming_commands_run_inside_a_pty() {
        let (sender, receiver) = async_channel::unbounded();
        run_streaming(
            ShellRequest {
                id: 6,
                command: "if [ -t 1 ]; then printf tty; else printf pipe; fi".to_owned(),
                cwd: std::env::current_dir().expect("current directory"),
            },
            sender,
        );
        let mut completion = None;
        while let Ok(event) = receiver.try_recv() {
            if let ShellStreamEvent::Finished(finished) = event {
                completion = Some(finished);
            }
        }

        assert_eq!(
            completion
                .expect("stream completion")
                .result
                .expect("stream result")
                .output,
            "tty"
        );
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
