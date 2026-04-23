use std::env;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn crew_readiness_lines(browser_native: bool, search_available: bool) -> Vec<String> {
    readiness_lines(browser_native, search_available)
}

pub fn worker_readiness_lines(browser_native: bool, search_available: bool) -> Vec<String> {
    readiness_lines(browser_native, search_available)
}

fn readiness_lines(browser_native: bool, search_available: bool) -> Vec<String> {
    let paths = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();

    let nmap = binary_in_paths("nmap", &paths);
    let sqlmap = binary_in_paths("sqlmap", &paths);
    let holehe = binary_in_paths("holehe", &paths);
    let sherlock = binary_in_paths("sherlock", &paths);
    let the_harvester = binary_in_paths("theHarvester", &paths);
    let python3 = binary_in_paths("python3", &paths);

    let mut lines = Vec::new();
    lines.push(if browser_native {
        "Browser runtime: native Chromium control is available for full navigation, click, typing, screenshot, and JS execution."
            .to_string()
    } else {
        "Browser runtime: read-only fallback only; navigate/get_content/get_links/get_forms work, but screenshot, click, type, and execute_js do not."
            .to_string()
    });
    lines.push(if search_available {
        "Web search: target-specific intelligence lookup is available.".to_string()
    } else {
        "Web search: unavailable; do not assume external intelligence lookups will work."
            .to_string()
    });
    lines.push(if nmap {
        "nmap: native binary present in PATH.".to_string()
    } else {
        "nmap: not found in PATH; wrapper falls back to mock output, so scan results are low-confidence."
            .to_string()
    });
    lines.push(if sqlmap {
        "sqlmap: native binary present in PATH.".to_string()
    } else {
        "sqlmap: not found in PATH; wrapper falls back to mock output, so SQLi findings need independent confirmation."
            .to_string()
    });
    lines.push(format!(
        "OSINT binaries: holehe={}, sherlock={}, theHarvester={}, python3={}.",
        availability_word(holehe),
        availability_word(sherlock),
        availability_word(the_harvester),
        availability_word(python3),
    ));
    lines
}

fn availability_word(value: bool) -> &'static str {
    if value { "ready" } else { "missing" }
}

fn binary_in_paths(binary: &str, paths: &[PathBuf]) -> bool {
    paths
        .iter()
        .any(|dir| is_executable_file(&dir.join(binary)))
}

fn is_executable_file(path: &PathBuf) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn binary_lookup_detects_executable_in_custom_path_list() {
        let temp_root = env::temp_dir().join(format!(
            "serpantoxide-cap-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_root).unwrap();
        let binary_path = temp_root.join("fakebin");
        fs::write(&binary_path, "#!/bin/sh\nexit 0\n").unwrap();

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&binary_path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&binary_path, permissions).unwrap();
        }

        assert!(binary_in_paths("fakebin", std::slice::from_ref(&temp_root)));
        assert!(!binary_in_paths(
            "missingbin",
            std::slice::from_ref(&temp_root)
        ));

        let _ = fs::remove_file(&binary_path);
        let _ = fs::remove_dir(&temp_root);
    }
}
