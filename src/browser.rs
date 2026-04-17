use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::Mutex;

pub struct NativeBrowserEngine {
    browser: Arc<Browser>,
    pub active_page: Arc<Mutex<Option<Page>>>,
}

impl NativeBrowserEngine {
    pub async fn launch() -> Result<Self, String> {
        let (browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .build()
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;

        tokio::spawn(async move { while let Some(_) = handler.next().await {} });

        Ok(Self {
            browser: Arc::new(browser),
            active_page: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn navigate(
        &self,
        url: &str,
        wait_for: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String, String> {
        let page = if let Some(existing) = self.current_page().await {
            existing
                .goto(url)
                .await
                .map_err(|e| e.to_string())?
                .wait_for_navigation()
                .await
                .map_err(|e| e.to_string())?
                .clone()
        } else {
            let page = self
                .browser
                .new_page(url)
                .await
                .map_err(|e| e.to_string())?;
            page.wait_for_navigation()
                .await
                .map_err(|e| e.to_string())?;
            page
        };

        if let Some(selector) = wait_for {
            self.wait_for_selector_on_page(&page, selector, timeout_ms)
                .await?;
        }

        let title = page
            .get_title()
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| "No Title".to_string());

        let mut active = self.active_page.lock().await;
        *active = Some(page.clone());

        Ok(format!("Navigated to: {}\nTitle: {}", url, title))
    }

    pub async fn screenshot(&self, url: Option<&str>, timeout_ms: u64) -> Result<String, String> {
        let page = self.page_for_read(url, timeout_ms).await?;
        let output_dir = Path::new("loot/artifacts/screenshots");
        std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let filename = format!("screenshot_{}_{}.png", timestamp, unique_suffix());
        let filepath = output_dir.join(filename);

        page.save_screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .full_page(true)
                .build(),
            &filepath,
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(format!("Screenshot saved to: {}", filepath.display()))
    }

    pub async fn get_content(&self, url: Option<&str>, timeout_ms: u64) -> Result<String, String> {
        let page = self.page_for_read(url, timeout_ms).await?;
        let text: String = page
            .evaluate("() => document.body ? document.body.innerText : ''")
            .await
            .map_err(|e| e.to_string())?
            .into_value()
            .map_err(|e| e.to_string())?;

        let text = if text.len() > 5_000 {
            format!("{}\n... (truncated)", &text[..5_000])
        } else {
            text
        };

        Ok(format!("Page content:\n{}", text))
    }

    pub async fn get_links(&self, url: Option<&str>, timeout_ms: u64) -> Result<String, String> {
        let page = self.page_for_read(url, timeout_ms).await?;
        let links: Vec<Value> = page
            .evaluate(
                "() => Array.from(document.querySelectorAll('a[href]')).map(a => ({ href: a.href, text: (a.innerText || '').trim() }))",
            )
            .await
            .map_err(|e| e.to_string())?
            .into_value()
            .map_err(|e| e.to_string())?;

        if links.is_empty() {
            return Ok("No links found on page".to_string());
        }

        let mut lines = vec!["Found links:".to_string()];
        for link in links.iter().take(50) {
            let href = link["href"].as_str().unwrap_or("");
            let text = link["text"].as_str().unwrap_or("").trim();
            let text = truncate(text, 50);
            lines.push(format!("  - [{}] {}", text, href));
        }
        if links.len() > 50 {
            lines.push(format!("  ... and {} more links", links.len() - 50));
        }
        Ok(lines.join("\n"))
    }

    pub async fn get_forms(&self, url: Option<&str>, timeout_ms: u64) -> Result<String, String> {
        let page = self.page_for_read(url, timeout_ms).await?;
        let forms: Vec<Value> = page
            .evaluate(
                "() => Array.from(document.querySelectorAll('form')).map(form => ({ action: form.action, method: form.method || 'GET', inputs: Array.from(form.querySelectorAll('input, textarea, select')).map(input => ({ name: input.name || 'unnamed', type: input.type || input.tagName.toLowerCase(), value: input.value || '' })) }))",
            )
            .await
            .map_err(|e| e.to_string())?
            .into_value()
            .map_err(|e| e.to_string())?;

        if forms.is_empty() {
            return Ok("No forms found on page".to_string());
        }

        let mut lines = vec!["Found forms:".to_string()];
        for (idx, form) in forms.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("Form {}:", idx + 1));
            lines.push(format!(
                "  Action: {}",
                form["action"].as_str().unwrap_or("N/A")
            ));
            lines.push(format!(
                "  Method: {}",
                form["method"].as_str().unwrap_or("GET")
            ));
            if let Some(inputs) = form["inputs"].as_array() {
                if !inputs.is_empty() {
                    lines.push("  Inputs:".to_string());
                    for input in inputs {
                        lines.push(format!(
                            "    - {} ({})",
                            input["name"].as_str().unwrap_or("unnamed"),
                            input["type"].as_str().unwrap_or("text")
                        ));
                    }
                }
            }
        }

        Ok(lines.join("\n"))
    }

    pub async fn click(
        &self,
        selector: &str,
        wait_for: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String, String> {
        let page = self.current_page().await.ok_or_else(|| {
            "No active page. Navigate first before attempting browser interactions.".to_string()
        })?;
        page.find_element(selector)
            .await
            .map_err(|e| e.to_string())?
            .click()
            .await
            .map_err(|e| e.to_string())?;

        if let Some(selector) = wait_for {
            self.wait_for_selector_on_page(&page, selector, timeout_ms)
                .await?;
        }

        Ok(format!("Clicked element: {}", selector))
    }

    pub async fn type_text(
        &self,
        selector: &str,
        text: &str,
        wait_for: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String, String> {
        let page = self.current_page().await.ok_or_else(|| {
            "No active page. Navigate first before attempting browser interactions.".to_string()
        })?;

        let escaped_selector = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        let escaped_text = serde_json::to_string(text).map_err(|e| e.to_string())?;
        let js = format!(
            "(() => {{ const el = document.querySelector({selector}); if (!el) return {{ok:false,error:'Selector not found'}}; el.focus(); if ('value' in el) {{ el.value = {text}; el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); return {{ok:true}}; }} return {{ok:false,error:'Element does not support value assignment'}}; }})()",
            selector = escaped_selector,
            text = escaped_text
        );
        let result: Value = page
            .evaluate(js)
            .await
            .map_err(|e| e.to_string())?
            .into_value()
            .map_err(|e| e.to_string())?;

        if !result["ok"].as_bool().unwrap_or(false) {
            return Err(result["error"]
                .as_str()
                .unwrap_or("Failed to type into selector")
                .to_string());
        }

        if let Some(selector) = wait_for {
            self.wait_for_selector_on_page(&page, selector, timeout_ms)
                .await?;
        }

        Ok(format!("Typed text into: {}", selector))
    }

    pub async fn execute_js(&self, javascript: &str) -> Result<String, String> {
        let page = self.current_page().await.ok_or_else(|| {
            "No active page. Navigate first before attempting browser interactions.".to_string()
        })?;
        let result: Value = page
            .evaluate(javascript)
            .await
            .map_err(|e| e.to_string())?
            .into_value()
            .map_err(|e| e.to_string())?;

        let rendered = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
        Ok(format!("JavaScript result:\n{}", rendered))
    }

    async fn page_for_read(&self, url: Option<&str>, timeout_ms: u64) -> Result<Page, String> {
        if let Some(url) = url {
            self.navigate(url, None, timeout_ms).await?;
        }
        self.current_page().await.ok_or_else(|| {
            "No active page. Navigate first before attempting browser interactions.".to_string()
        })
    }

    async fn current_page(&self) -> Option<Page> {
        self.active_page.lock().await.clone()
    }

    async fn wait_for_selector_on_page(
        &self,
        page: &Page,
        selector: &str,
        timeout_ms: u64,
    ) -> Result<(), String> {
        let start = tokio::time::Instant::now();
        let timeout = Duration::from_millis(timeout_ms.max(1));
        while start.elapsed() < timeout {
            if page.find_element(selector).await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Err(format!(
            "Timed out waiting for selector '{}' after {}ms",
            selector, timeout_ms
        ))
    }
}

fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        .to_string()
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect()
    }
}
