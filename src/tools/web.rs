use async_trait::async_trait;
use serde_json::{json, Value};

use super::Tool;
use super::executor::maybe_truncate_chars;
use crate::types::*;

// ─── web_fetch ────────────────────────────────────────────────────────────

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "Fetch a URL and return its content as plain text. HTML is converted to readable text."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "max_length": {
                    "type": "integer",
                    "description": "Optional maximum number of Unicode characters to return. Omit or set 0 for full fetched content."
                }
            },
            "required": ["url"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Network
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let url = match input["url"].as_str() {
            Some(u) => u,
            None => return ToolOutput::error("Missing 'url' parameter"),
        };
        let max_length = input["max_length"].as_u64().map(|n| n as usize);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("forge-osh/0.1")
            .build();

        let client = match client {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("Failed to create client: {e}")),
        };

        match client.get(url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    return ToolOutput::error(format!(
                        "HTTP error {}: {}",
                        response.status().as_u16(),
                        response.status().canonical_reason().unwrap_or("Unknown")
                    ));
                }

                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                match response.text().await {
                    Ok(body) => {
                        let text = if content_type.contains("text/html") {
                            // Convert HTML to plain text
                            html2text::from_read(body.as_bytes(), 80)
                        } else {
                            body
                        };

                        ToolOutput::success(maybe_truncate_chars(text, max_length))
                    }
                    Err(e) => ToolOutput::error(format!("Failed to read response: {e}")),
                }
            }
            Err(e) => ToolOutput::error(format!("Request failed: {e}")),
        }
    }
}

// ─── web_search ───────────────────────────────────────────────────────────

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web using DuckDuckGo. Returns titles, URLs, and snippets."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "max_results": { "type": "integer", "default": 5 }
            },
            "required": ["query"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Network
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let query = match input["query"].as_str() {
            Some(q) => q,
            None => return ToolOutput::error("Missing 'query' parameter"),
        };
        let max_results = input["max_results"].as_u64().unwrap_or(5) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("forge-osh/0.1")
            .build();

        let client = match client {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("Failed to create client: {e}")),
        };

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        match client.get(&url).send().await {
            Ok(response) => match response.text().await {
                Ok(html) => {
                    let results = parse_ddg_results(&html, max_results);
                    if results.is_empty() {
                        ToolOutput::success(format!("No results found for: {query}"))
                    } else {
                        ToolOutput::success(results.join("\n\n"))
                    }
                }
                Err(e) => ToolOutput::error(format!("Failed to read response: {e}")),
            },
            Err(e) => ToolOutput::error(format!("Search failed: {e}")),
        }
    }
}

/// Very simple DDG HTML result parser
fn parse_ddg_results(html: &str, max_results: usize) -> Vec<String> {
    let mut results = Vec::new();

    // Look for result links and snippets in the HTML
    // DDG HTML results have class="result__a" for titles and class="result__snippet" for descriptions
    for segment in html.split("class=\"result__a\"") {
        if results.len() >= max_results {
            break;
        }
        if results.is_empty() && !segment.contains("href=") {
            // First split is before any result
            continue;
        }

        // Extract href
        let href = segment
            .split("href=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("(no url)");

        // Extract title text (between > and </a>)
        let title = segment
            .split('>')
            .nth(1)
            .and_then(|s| s.split("</a>").next())
            .map(html_entities_decode)
            .unwrap_or_else(|| "(no title)".to_string());

        // Extract snippet
        let snippet = segment
            .split("class=\"result__snippet\"")
            .nth(1)
            .and_then(|s| s.split('>').nth(1))
            .and_then(|s| s.split("</").next())
            .map(html_entities_decode)
            .unwrap_or_default();

        // Clean up the DDG redirect URL
        let clean_url = if href.contains("uddg=") {
            href.split("uddg=")
                .nth(1)
                .and_then(|u| u.split('&').next())
                .map(|u| urlencoding::decode(u).unwrap_or_default().to_string())
                .unwrap_or_else(|| href.to_string())
        } else {
            href.to_string()
        };

        results.push(format!(
            "{}. {}\n   {}\n   {}",
            results.len() + 1,
            title,
            clean_url,
            snippet
        ));
    }

    results
}

fn html_entities_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<em>", "")
        .replace("</em>", "")
}

// Simple percent-encoding for query strings
mod urlencoding {
    use std::borrow::Cow;

    pub fn encode(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        for byte in s.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                b' ' => result.push('+'),
                _ => {
                    result.push('%');
                    result.push_str(&format!("{byte:02X}"));
                }
            }
        }
        result
    }

    pub fn decode(s: &str) -> Result<Cow<'_, str>, std::string::FromUtf8Error> {
        let mut result = Vec::new();
        let mut chars = s.bytes();
        while let Some(b) = chars.next() {
            if b == b'%' {
                let hi = chars.next().unwrap_or(0);
                let lo = chars.next().unwrap_or(0);
                let hex = [hi, lo];
                if let Ok(s) = std::str::from_utf8(&hex) {
                    if let Ok(val) = u8::from_str_radix(s, 16) {
                        result.push(val);
                        continue;
                    }
                }
                result.push(b'%');
                result.push(hi);
                result.push(lo);
            } else if b == b'+' {
                result.push(b' ');
            } else {
                result.push(b);
            }
        }
        String::from_utf8(result).map(Cow::Owned)
    }
}
