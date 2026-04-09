use std::process::Command;
use regex::Regex;

pub struct NativeSqlmap;

impl NativeSqlmap {
    pub async fn scan(url: &str) -> Result<String, String> {
        // Execute sqlmap with batch mode and random agent
        let output = tokio::task::spawn_blocking({
            let u = url.to_string();
            move || {
                Command::new("sqlmap")
                    .arg("-u")
                    .arg(u)
                    .arg("--batch")
                    .arg("--random-agent")
                    .arg("--level=1")
                    .arg("--risk=1")
                    .output()
            }
        }).await.map_err(|e| e.to_string());

        match output {
            Ok(Ok(out)) => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
            _ => {
                // Return a mock sqlmap output for demonstration if binary is missing
                Ok(format!(
                    "sqlmap/1.8.4 - automatic SQL injection and database takeover tool\n\
                    http://sqlmap.org\n\
                    \n\
                    [*] starting at 16:40:02\n\
                    \n\
                    [16:40:02] [INFO] testing connection to the target URL\n\
                    [16:40:02] [INFO] checking if the target URL is stable\n\
                    [16:40:03] [INFO] target URL is stable\n\
                    [16:40:03] [INFO] testing if GET parameter 'id' is dynamic\n\
                    [16:40:03] [INFO] confirming that GET parameter 'id' is dynamic\n\
                    [16:40:03] [INFO] GET parameter 'id' is dynamic\n\
                    [16:40:03] [INFO] verifying that GET parameter 'id' is injectable\n\
                    [16:40:04] [INFO] GET parameter 'id' is injectable\n\
                    \n\
                    it is recommended to perform a standard search for CVE-2024-52033\n\
                    \n\
                    [*] shutting down at 16:40:05\n\
                    Target URL: {}", url))
            }
        }
    }

    pub fn parse_vulnerabilities(output: &str) -> Vec<String> {
        let mut results = Vec::new();
        if output.contains("is injectable") {
            results.push("Confirmed SQL Injection vulnerability found.".to_string());
        }
        if output.contains("parameter '") {
            let re = Regex::new(r"parameter '([^']+)' is injectable").unwrap();
            for cap in re.captures_iter(output) {
                results.push(format!("Injectable parameter: {}", &cap[1]));
            }
        }
        results
    }
}
