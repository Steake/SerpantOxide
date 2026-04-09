use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

pub struct NativeTerminal;

impl NativeTerminal {
    pub async fn execute(command: &str, timeout_secs: u64) -> Result<String, String> {
        let cmd_parts: Vec<&str> = command.split_whitespace().collect();
        if cmd_parts.is_empty() {
            return Err("Empty command provided".to_string());
        }

        let mut child = Command::new(cmd_parts[0]);
        if cmd_parts.len() > 1 {
            child.args(&cmd_parts[1..]);
        }

        let output_future = tokio::task::spawn_blocking(move || {
            child.output()
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
