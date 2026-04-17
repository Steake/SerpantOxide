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

            if inputs.is_some() {
                child.stdin(Stdio::piped());
            }

            if let Some(dir) = working_dir {
                child.current_dir(dir);
            }

            let mut child = child.spawn()?;

            if let Some(input) = inputs {
                if let Some(mut stdin) = child.stdin.take() {
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
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
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
