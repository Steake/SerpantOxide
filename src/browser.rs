use chromiumoxide::browser::{Browser, BrowserConfig};
// use chromiumoxide::handler::Handler;
use chromiumoxide::Page;
use std::sync::Arc;
use tokio::sync::Mutex;
use futures::StreamExt;

pub struct NativeBrowserEngine {
    _browser: Arc<Browser>,
    pub active_page: Arc<Mutex<Option<Page>>>,
}

impl NativeBrowserEngine {
    pub async fn launch() -> Result<Self, String> {
        // Build isolated headless CDP connection bypassing nodejs
        let (browser, mut handler) = Browser::launch(BrowserConfig::builder().build().map_err(|e| e.to_string())?)
            .await.map_err(|e| e.to_string())?;

        // Background handler for chromiumoxide continuous event loop
        tokio::spawn(async move {
            while let Some(_) = handler.next().await {
                // Background driving engine
            }
        });

        Ok(NativeBrowserEngine {
            _browser: Arc::new(browser),
            active_page: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn action(&self, action: &str, target: &str) -> Result<String, String> {
        if action == "navigate" || action == "goto" {
            let page = self._browser.new_page(target).await.map_err(|e| e.to_string())?;
            // We use standard chromiumoxide CDP hooks instead of playwright
            let _ = page.wait_for_navigation().await.map_err(|e| e.to_string())?;
            let title = page.get_title().await.map_err(|e| e.to_string())?.unwrap_or_else(|| "No Title".to_string());
            
            let mut active = self.active_page.lock().await;
            *active = Some(page);
            
            return Ok(format!("Chromiumoxide CDP Navigated to {} -> {}", target, title));
        } else if action == "get_content" {
            let active = self.active_page.lock().await;
            if let Some(page) = &*active {
                let content = page.content().await.map_err(|e| e.to_string())?;
                let snippet = if content.len() > 150 { &content[..150] } else { &content };
                return Ok(format!("Scraped {} bytes! Snippet: {}...", content.len(), snippet));
            }
            return Ok("Error: No active page to scrape. Navigate first.".to_string());
        }

        Ok("Unknown Action".to_string())
    }
}
