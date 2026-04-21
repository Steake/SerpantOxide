mod browser;
mod config;
mod events;
mod evm_chain;
mod gpui_app;
mod graph;
mod hosting;
mod image_gen;
mod llm;
mod nmap;
mod notes;
mod osint;
mod orchestrator;
mod pool;
mod prompts;
mod runtime;
mod sqlmap;
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
    dotenv::dotenv().ok();

    let frontend = selected_frontend();
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let runtime = tokio_runtime
        .block_on(RuntimeService::launch())
        .map_err(std::io::Error::other)?;

    match frontend {
        FrontendMode::Tui => {
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
            gpui_app::run(runtime, tokio_runtime.handle().clone())
                .map_err(std::io::Error::other)?;
        }
    }

    Ok(())
}

fn selected_frontend() -> FrontendMode {
    let args = std::env::args().collect::<Vec<_>>();
    #[cfg(target_os = "macos")]
    if args.iter().any(|arg| arg == "--gpui") {
        return FrontendMode::Gpui;
    }

    if args.iter().any(|arg| arg == "--tui") {
        return FrontendMode::Tui;
    }

    FrontendMode::Tui
}
