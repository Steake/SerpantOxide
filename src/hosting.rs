use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

struct HostingState {
    process: Option<Child>,
    content_path: Option<String>,
}

fn hosting_state() -> &'static Mutex<HostingState> {
    static STATE: OnceLock<Mutex<HostingState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(HostingState {
            process: None,
            content_path: None,
        })
    })
}

pub async fn control(action: &str, content_path: Option<&str>) -> Result<String, String> {
    match action {
        "start" => start(content_path),
        "stop" => stop(),
        "status" => status(),
        other => Err(format!("Unknown hosting action: {}", other)),
    }
}

fn start(content_path: Option<&str>) -> Result<String, String> {
    let content_path =
        content_path.ok_or_else(|| "content_path is required for start".to_string())?;
    let path = Path::new(content_path);
    if !path.exists() {
        return Err(format!("Content path does not exist: {}", content_path));
    }

    let directory = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .ok_or_else(|| "Could not resolve content directory".to_string())?
            .to_path_buf()
    };

    let mut state = hosting_state()
        .lock()
        .map_err(|_| "Hosting state poisoned".to_string())?;
    if state.process.is_some() {
        return Ok(format!(
            "Server already running.\nPublic URL: http://127.0.0.1:8000\nContent: {}",
            state.content_path.clone().unwrap_or_default()
        ));
    }

    let child = Command::new("python3")
        .arg("-m")
        .arg("http.server")
        .arg("8000")
        .arg("--directory")
        .arg(directory)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start hosting server: {}", e))?;

    state.process = Some(child);
    state.content_path = Some(content_path.to_string());

    Ok(format!(
        "Server STARTED. Hosting: {}\nPublic URL: http://127.0.0.1:8000",
        content_path
    ))
}

fn stop() -> Result<String, String> {
    let mut state = hosting_state()
        .lock()
        .map_err(|_| "Hosting state poisoned".to_string())?;
    if let Some(mut child) = state.process.take() {
        let _ = child.kill();
        let _ = child.wait();
        state.content_path = None;
        Ok("Server STOPPED.".to_string())
    } else {
        Ok("Server already stopped.".to_string())
    }
}

fn status() -> Result<String, String> {
    let mut state = hosting_state()
        .lock()
        .map_err(|_| "Hosting state poisoned".to_string())?;
    let running = if let Some(child) = state.process.as_mut() {
        child
            .try_wait()
            .map_err(|e| format!("Failed to inspect hosting process: {}", e))?
            .is_none()
    } else {
        false
    };

    if !running {
        state.process = None;
    }

    Ok(format!(
        "Server Status: {}\nPublic URL: {}\nContent: {}",
        if running { "RUNNING" } else { "STOPPED" },
        if running {
            "http://127.0.0.1:8000"
        } else {
            "N/A"
        },
        state
            .content_path
            .clone()
            .unwrap_or_else(|| "N/A".to_string())
    ))
}
