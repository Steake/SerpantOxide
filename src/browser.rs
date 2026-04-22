use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use reqwest::{Client, Url};
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use tokio::sync::Mutex;

pub struct NativeBrowserEngine {
    browser: Arc<Browser>,
    pub active_page: Arc<Mutex<Option<Page>>>,
}

#[derive(Clone)]
struct FallbackPageSnapshot {
    url: String,
    html: String,
    title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FallbackFormInput {
    name: String,
    kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FallbackForm {
    action: String,
    method: String,
    inputs: Vec<FallbackFormInput>,
}

pub struct ReadOnlyBrowserFallback {
    client: Client,
    active_page: Arc<Mutex<Option<FallbackPageSnapshot>>>,
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

impl ReadOnlyBrowserFallback {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            active_page: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn navigate(
        &self,
        url: &str,
        wait_for: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String, String> {
        let snapshot = self.fetch_page(url, timeout_ms).await?;
        let mut active = self.active_page.lock().await;
        *active = Some(snapshot.clone());
        drop(active);

        let mut lines = vec![
            format!("Navigated to: {}", snapshot.url),
            format!("Title: {}", snapshot.title),
            "Mode: HTTP fallback (native browser engine unavailable)".to_string(),
        ];
        if let Some(selector) = wait_for {
            lines.push(format!(
                "Note: wait_for '{}' was ignored because the HTTP fallback does not execute DOM waits.",
                selector
            ));
        }
        Ok(lines.join("\n"))
    }

    pub async fn get_content(&self, url: Option<&str>, timeout_ms: u64) -> Result<String, String> {
        let snapshot = self.page_for_read(url, timeout_ms).await?;
        let content = extract_body_text(&snapshot.html);
        let content = if content.len() > 5_000 {
            format!("{}\n... (truncated)", &content[..5_000])
        } else {
            content
        };

        Ok(format!(
            "Page content:\n{}\n\nMode: HTTP fallback (native browser engine unavailable)",
            content
        ))
    }

    pub async fn get_links(&self, url: Option<&str>, timeout_ms: u64) -> Result<String, String> {
        let snapshot = self.page_for_read(url, timeout_ms).await?;
        let base_url = Url::parse(&snapshot.url).map_err(|e| e.to_string())?;
        let links = extract_links(&snapshot.html, &base_url);

        if links.is_empty() {
            return Ok("No links found on page".to_string());
        }

        let mut lines = vec![
            "Found links:".to_string(),
            "Mode: HTTP fallback (native browser engine unavailable)".to_string(),
        ];
        for (text, href) in links.iter().take(50) {
            lines.push(format!("  - [{}] {}", truncate(text, 50), href));
        }
        if links.len() > 50 {
            lines.push(format!("  ... and {} more links", links.len() - 50));
        }
        Ok(lines.join("\n"))
    }

    pub async fn get_forms(&self, url: Option<&str>, timeout_ms: u64) -> Result<String, String> {
        let snapshot = self.page_for_read(url, timeout_ms).await?;
        let base_url = Url::parse(&snapshot.url).map_err(|e| e.to_string())?;
        let forms = extract_forms(&snapshot.html, &base_url);

        if forms.is_empty() {
            return Ok("No forms found on page".to_string());
        }

        let mut lines = vec![
            "Found forms:".to_string(),
            "Mode: HTTP fallback (native browser engine unavailable)".to_string(),
        ];
        for (idx, form) in forms.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("Form {}:", idx + 1));
            lines.push(format!("  Action: {}", form.action));
            lines.push(format!("  Method: {}", form.method));
            if !form.inputs.is_empty() {
                lines.push("  Inputs:".to_string());
                for input in &form.inputs {
                    lines.push(format!("    - {} ({})", input.name, input.kind));
                }
            }
        }

        Ok(lines.join("\n"))
    }

    pub fn unsupported_action_message(&self, action: &str) -> String {
        format!(
            "Browser action '{}' requires the native Chromium engine. The HTTP fallback only supports navigate, get_content, get_links, and get_forms.",
            action
        )
    }

    async fn page_for_read(
        &self,
        url: Option<&str>,
        timeout_ms: u64,
    ) -> Result<FallbackPageSnapshot, String> {
        if let Some(url) = url {
            return self.fetch_and_store(url, timeout_ms).await;
        }

        self.active_page.lock().await.clone().ok_or_else(|| {
            "No active page. Navigate first or provide a url for browser inspection.".to_string()
        })
    }

    async fn fetch_and_store(
        &self,
        url: &str,
        timeout_ms: u64,
    ) -> Result<FallbackPageSnapshot, String> {
        let snapshot = self.fetch_page(url, timeout_ms).await?;
        let mut active = self.active_page.lock().await;
        *active = Some(snapshot.clone());
        Ok(snapshot)
    }

    async fn fetch_page(&self, url: &str, timeout_ms: u64) -> Result<FallbackPageSnapshot, String> {
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_millis(timeout_ms.max(1)))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;

        let final_url = response.url().to_string();
        let html = response.text().await.map_err(|e| e.to_string())?;
        let title = extract_title(&html).unwrap_or_else(|| "No Title".to_string());

        Ok(FallbackPageSnapshot {
            url: final_url,
            html,
            title,
        })
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

fn extract_title(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let title_selector = selector("title");
    document
        .select(&title_selector)
        .next()
        .map(element_text)
        .filter(|value| !value.is_empty())
}

fn extract_body_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let body_selector = selector("body");
    let text = document
        .select(&body_selector)
        .next()
        .map(element_text)
        .unwrap_or_default();

    if text.is_empty() {
        "No readable body content found".to_string()
    } else {
        text
    }
}

fn extract_links(html: &str, base_url: &Url) -> Vec<(String, String)> {
    let document = Html::parse_document(html);
    let link_selector = selector("a[href]");
    document
        .select(&link_selector)
        .filter_map(|link| {
            let href = link.value().attr("href")?;
            let resolved = base_url
                .join(href)
                .map(|url| url.to_string())
                .unwrap_or_else(|_| href.to_string());
            let text = element_text(link);
            Some((
                if text.is_empty() {
                    "link".to_string()
                } else {
                    text
                },
                resolved,
            ))
        })
        .collect()
}

fn extract_forms(html: &str, base_url: &Url) -> Vec<FallbackForm> {
    let document = Html::parse_document(html);
    let form_selector = selector("form");
    let field_selector = selector("input, textarea, select");

    document
        .select(&form_selector)
        .map(|form| {
            let action_attr = form.value().attr("action").unwrap_or("");
            let action = if action_attr.is_empty() {
                base_url.to_string()
            } else {
                base_url
                    .join(action_attr)
                    .map(|url| url.to_string())
                    .unwrap_or_else(|_| action_attr.to_string())
            };
            let method = form.value().attr("method").unwrap_or("GET").to_uppercase();
            let inputs = form
                .select(&field_selector)
                .map(|field| FallbackFormInput {
                    name: field.value().attr("name").unwrap_or("unnamed").to_string(),
                    kind: field
                        .value()
                        .attr("type")
                        .unwrap_or(field.value().name())
                        .to_string(),
                })
                .collect::<Vec<_>>();

            FallbackForm {
                action,
                method,
                inputs,
            }
        })
        .collect()
}

fn element_text(element: ElementRef<'_>) -> String {
    element
        .text()
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn selector(pattern: &str) -> Selector {
    Selector::parse(pattern).expect("valid CSS selector")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_with_resolved_urls() {
        let html = r#"
            <html>
              <body>
                <a href="/apply">Apply now</a>
                <a href="https://example.org/help">Help</a>
              </body>
            </html>
        "#;

        let links = extract_links(
            html,
            &Url::parse("https://www.evisagov.co/official/en/").unwrap(),
        );

        assert_eq!(
            links[0],
            (
                "Apply now".to_string(),
                "https://www.evisagov.co/apply".to_string()
            )
        );
        assert_eq!(
            links[1],
            ("Help".to_string(), "https://example.org/help".to_string())
        );
    }

    #[test]
    fn extracts_forms_and_body_text() {
        let html = r#"
            <html>
              <head><title>Visa form</title></head>
              <body>
                <main>
                  <h1>Request visa</h1>
                  <form action="/submit" method="post">
                    <input name="passport" type="text" />
                    <textarea name="notes"></textarea>
                    <select name="country"></select>
                  </form>
                </main>
              </body>
            </html>
        "#;

        let forms = extract_forms(html, &Url::parse("https://example.com/apply").unwrap());

        assert_eq!(extract_title(html).as_deref(), Some("Visa form"));
        assert!(extract_body_text(html).contains("Request visa"));
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].action, "https://example.com/submit");
        assert_eq!(forms[0].method, "POST");
        assert_eq!(
            forms[0].inputs,
            vec![
                FallbackFormInput {
                    name: "passport".to_string(),
                    kind: "text".to_string()
                },
                FallbackFormInput {
                    name: "notes".to_string(),
                    kind: "textarea".to_string()
                },
                FallbackFormInput {
                    name: "country".to_string(),
                    kind: "select".to_string()
                }
            ]
        );
    }
}
