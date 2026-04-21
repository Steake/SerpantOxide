use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use tokio::time::timeout;

pub struct NativeTerminal;

impl NativeTerminal {
    pub async fn execute_with_options(
        command: &str,
        timeout_secs: u64,
        working_dir: Option<&str>,
        inputs: Option<&str>,
        privileged: bool,
    ) -> Result<String, String> {
        let shell_command = if privileged {
            format!("sudo -S sh -lc '{}'", escape_single_quotes(command))
        } else {
            format!("sh -lc '{}'", escape_single_quotes(command))
        };

        let working_dir = working_dir.map(ToString::to_string);
        let inputs = inputs.map(ToString::to_string);

        let output_future = tokio::task::spawn_blocking(move || {
            let mut child = Command::new("sh");
            child.arg("-lc").arg(shell_command);
            child.stdout(Stdio::piped()).stderr(Stdio::piped());

            if inputs.is_some() || privileged {
                child.stdin(Stdio::piped());
            } else {
                child.stdin(Stdio::null());
            }

            if let Some(dir) = working_dir {
                child.current_dir(dir);
            }

            let mut child = child.spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                if let Some(input) = inputs {
                    stdin.write_all(input.as_bytes())?;
                    if !input.ends_with('\n') {
                        stdin.write_all(b"\n")?;
                    }
                }
            }

            child.wait_with_output()
        });

        match timeout(Duration::from_secs(timeout_secs), output_future).await {
            Ok(Ok(Ok(output))) => {
                let stdout = sanitize_terminal_output(&String::from_utf8_lossy(&output.stdout));
                let stderr = sanitize_terminal_output(&String::from_utf8_lossy(&output.stderr));
                let exit_code = output.status.code().unwrap_or(-1);

                Ok(format!(
                    "Exit Code: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                    exit_code, stdout, stderr
                ))
            }
            Ok(Ok(Err(e))) => Err(format!("Command failed: {}", e)),
            Ok(Err(e)) => Err(format!("Task panic: {}", e)),
            Err(_) => Err(format!("Command timed out after {}s", timeout_secs)),
        }
    }
}

fn escape_single_quotes(input: &str) -> String {
    input.replace('\'', "'\"'\"'")
}

fn sanitize_terminal_output(input: &str) -> String {
    let mut sanitized = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
                continue;
            }

            if matches!(chars.peek(), Some(']')) {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{07}' {
                        break;
                    }
                    if next == '\u{1b}' && matches!(chars.peek(), Some('\\')) {
                        let _ = chars.next();
                        break;
                    }
                }
                continue;
            }

            continue;
        }

        match ch {
            '\n' | '\r' | '\t' => sanitized.push(ch),
            c if c.is_control() => {}
            _ => sanitized.push(ch),
        }
    }

    sanitized
}
