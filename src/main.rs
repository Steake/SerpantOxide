mod browser;
mod config;
mod events;
mod evm_chain;
mod gpui_app;
mod graph;
mod hosting;
mod image_gen;
mod llm;
mod mission;
mod nmap;
mod notes;
mod orchestrator;
mod osint;
mod pool;
mod prompts;
mod runtime;
mod sqlmap;
mod startup_trace;
mod terminal;
mod tui;
mod web_search;
mod worker_agent;

use runtime::RuntimeService;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrontendMode {
    Tui,
    #[cfg(target_os = "macos")]
    Gpui,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = std::env::args().collect::<Vec<_>>();
    startup_trace::session_start(&args);
    startup_trace::log("main", "starting process");
    dotenv::dotenv().ok();
    startup_trace::log("main", "dotenv loaded");

    let frontend = selected_frontend(&args);
    startup_trace::log("main", format!("selected frontend: {:?}", frontend));
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    startup_trace::log("main", "tokio runtime built");

    startup_trace::log("main", "launching runtime service");
    let runtime = tokio_runtime
        .block_on(RuntimeService::launch())
        .map_err(std::io::Error::other)?;
    startup_trace::log("main", "runtime service launched");

    match frontend {
        FrontendMode::Tui => {
            startup_trace::log("main", "entering TUI");
            startup_trace::disable_stderr();
            tokio_runtime
                .block_on(tui::run_tui(
                    runtime.subscribe(),
                    runtime.command_sender(),
                    runtime.graph(),
                    runtime.llm_engine(),
                    runtime.target_shared(),
                    runtime.worker_pool(),
                ))
                .map_err(std::io::Error::other)?;
        }
        #[cfg(target_os = "macos")]
        FrontendMode::Gpui => {
            startup_trace::log("main", "entering GPUI");
            startup_trace::disable_stderr();
            gpui_app::run(runtime, tokio_runtime.handle().clone())
                .map_err(std::io::Error::other)?;
        }
    }

    startup_trace::log("main", "frontend exited cleanly");
    Ok(())
}

fn selected_frontend(args: &[String]) -> FrontendMode {
    #[cfg(target_os = "macos")]
    if args.iter().any(|arg| arg == "--gpui") {
        return FrontendMode::Gpui;
    }

    if args.iter().any(|arg| arg == "--tui") {
        return FrontendMode::Tui;
    }

    FrontendMode::Tui
}
