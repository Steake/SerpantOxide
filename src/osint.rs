use crate::terminal::NativeTerminal;

pub async fn run(tool: &str, target: &str) -> Result<String, String> {
    match tool {
        "holehe" => {
            let command = format!("holehe {} --only-used --no-color", shell_escape(target));
            run_with_fallback(&command, &format!("python3 -m {}", command)).await
        }
        "sherlock" => {
            let command = format!(
                "sherlock {} --print-found --timeout 5",
                shell_escape(target)
            );
            run_with_fallback(&command, &format!("python3 -m {}", command)).await
        }
        "theHarvester" => {
            let command = format!(
                "theHarvester -d {} -b bing,duckduckgo,crtsh,hackertarget,otx,rapiddns -l 200",
                shell_escape(target)
            );
            NativeTerminal::execute_with_options(&command, 300, None, None, false).await
        }
        other => Err(format!("Unsupported OSINT tool: {}", other)),
    }
}

async fn run_with_fallback(primary: &str, fallback: &str) -> Result<String, String> {
    let primary_result =
        NativeTerminal::execute_with_options(primary, 300, None, None, false).await?;
    if primary_result.contains("Command failed:") || primary_result.contains("not found") {
        NativeTerminal::execute_with_options(fallback, 300, None, None, false).await
    } else {
        Ok(primary_result)
    }
}

fn shell_escape(value: &str) -> String {
    value.replace('\'', "'\"'\"'")
}
