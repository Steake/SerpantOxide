use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static STDERR_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn session_start(args: &[String]) {
    let path = log_path();
    let line = format!(
        "\n===== serpantoxide startup pid={} args={:?} log={} =====",
        process::id(),
        args,
        path.display()
    );
    write_line("SESSION", "main", &line);
}

pub fn disable_stderr() {
    STDERR_ENABLED.store(false, Ordering::Relaxed);
}

pub fn log(stage: &str, message: impl AsRef<str>) {
    write_line("INFO", stage, message.as_ref());
}

pub fn log_error(stage: &str, message: impl AsRef<str>) {
    write_line("ERROR", stage, message.as_ref());
}

fn write_line(level: &str, stage: &str, message: &str) {
    let timestamp = timestamp_secs();
    let line = format!(
        "[{}][pid={}][{}][{}] {}\n",
        timestamp,
        process::id(),
        level,
        stage,
        message
    );

    if STDERR_ENABLED.load(Ordering::Relaxed) {
        let _ = std::io::stderr().write_all(line.as_bytes());
        let _ = std::io::stderr().flush();
    }

    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

fn log_path() -> PathBuf {
    if let Ok(path) = std::env::var("SERPANTOXIDE_STARTUP_LOG") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    PathBuf::from("/tmp/serpantoxide-startup.log")
}

fn timestamp_secs() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("{}.{}", duration.as_secs(), duration.subsec_millis()),
        Err(_) => "time-error".to_string(),
    }
}
