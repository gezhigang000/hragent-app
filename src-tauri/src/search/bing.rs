//! Bing search client — scrapes Bing CN search results (no API key required).
//!
//! Uses `cn.bing.com` which is accessible from mainland China.
//! Parses HTML search results into structured data.

use anyhow::{anyhow, Result};
use log::{info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// A single search result parsed from Bing HTML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BingResult {
    pub title: String,
    pub url: String,
    pub content: String,
}

/// Bing search client (HTML scraping, no API key needed).
pub struct BingClient {
    client: Client,
}

impl BingClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::limited(3))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Search via Bing CN and parse HTML results.
    pub async fn search(&self, query: &str, max_results: u32) -> Result<Vec<BingResult>> {
        let url = "https://cn.bing.com/search";

        let resp = self
            .client
            .get(url)
            .query(&[
                ("q", query),
                ("count", &max_results.to_string()),
            ])
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("Bing search returned HTTP {}", status.as_u16()));
        }

        let html = resp.text().await?;
        let results = parse_bing_html(&html, max_results as usize);

        if results.is_empty() {
            warn!("Bing search returned HTML but no results could be parsed (html_len={})", html.len());
        } else {
            info!("Bing search returned {} results", results.len());
        }

        Ok(results)
    }
}

/// Parse Bing search results from HTML.
///
/// Bing organic results live inside `<li class="b_algo">` elements.
/// Each contains:
/// - An `<a href="...">` with the result URL (first http(s) link after h2)
/// - A `<div class="b_caption">` with a `<p>` snippet
fn parse_bing_html(html: &str, max_results: usize) -> Vec<BingResult> {
    let mut results = Vec::new();

    // Split by b_algo list items
    let parts: Vec<&str> = html.split("<li class=\"b_algo\"").collect();

    // Skip the first part (content before the first result)
    for part in parts.iter().skip(1).take(max_results) {
        if let Some(result) = parse_single_result(part) {
            results.push(result);
        }
    }

    results
}

/// Parse a single search result from a `<li class="b_algo">` HTML fragment.
fn parse_single_result(html: &str) -> Option<BingResult> {
    // Extract URL: first <a> tag with an http(s) href
    let url = extract_first_url(html)?;

    // Extract title: text inside the <a> tag that contains the URL
    let title = extract_title(html, &url).unwrap_or_default();
    if title.is_empty() {
        return None;
    }

    // Extract snippet from <div class="b_caption">...<p>...</p>
    let snippet = extract_snippet(html);

    Some(BingResult {
        title,
        url,
        content: snippet,
    })
}

/// Extract the first http(s) URL from an `<a>` tag.
fn extract_first_url(html: &str) -> Option<String> {
    // Look for href="https://..." or href="http://..."
    let mut search_from = 0;
    while let Some(href_pos) = html[search_from..].find("href=\"http") {
        let abs_pos = search_from + href_pos + 6; // skip `href="`
        if let Some(end) = html[abs_pos..].find('"') {
            let url = &html[abs_pos..abs_pos + end];
            // Skip Bing internal URLs
            if !url.contains("bing.com/") && !url.contains("microsoft.com/") {
                return Some(decode_html_entities(url));
            }
        }
        search_from = search_from + href_pos + 1;
    }
    None
}

/// Extract the title text from the `<a>` tag that contains the given URL.
fn extract_title(html: &str, url: &str) -> Option<String> {
    // Primary: look for title inside <h2>...<a href="url">title</a>...</h2>
    // Bing uses <h2 class=""> so we match <h2 with any attributes
    if let Some(h2_start) = html.find("<h2") {
        if let Some(h2_end) = html[h2_start..].find("</h2>") {
            let h2_content = &html[h2_start..h2_start + h2_end];
            // Find <a> tags inside h2 and extract their text
            let mut pos = 0;
            while let Some(a_start) = h2_content[pos..].find("<a") {
                let abs = pos + a_start;
                if let Some(gt) = h2_content[abs..].find('>') {
                    let after_gt = abs + gt + 1;
                    if let Some(close) = h2_content[after_gt..].find("</a>") {
                        let text = strip_html_tags(&h2_content[after_gt..after_gt + close]);
                        let text = decode_html_entities(&text).trim().to_string();
                        if !text.is_empty() && text.len() > 2 {
                            return Some(text);
                        }
                        pos = after_gt + close + 4;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }

    // Fallback: find the <a> tag containing the URL and extract its text
    let url_needle = if let Some(idx) = url.find("://") {
        &url[idx + 3..] // Skip scheme for matching
    } else {
        url
    };

    let needle = &url_needle[..url_needle.len().min(30)];
    if let Some(a_pos) = html.find(needle) {
        let before = &html[..a_pos];
        if let Some(a_start) = before.rfind("<a") {
            if let Some(a_end) = html[a_pos..].find("</a>") {
                let a_content = &html[a_start..a_pos + a_end];
                if let Some(gt_pos) = a_content.find('>') {
                    let inner = &a_content[gt_pos + 1..];
                    let text = strip_html_tags(inner);
                    let text = decode_html_entities(&text).trim().to_string();
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
            }
        }
    }

    None
}

/// Extract the snippet text from `<div class="b_caption">...<p>...</p>`.
fn extract_snippet(html: &str) -> String {
    // Try b_caption first
    if let Some(cap_start) = html.find("class=\"b_caption\"") {
        let after_cap = &html[cap_start..];
        // Find the first <p> inside the caption
        if let Some(p_start) = after_cap.find("<p") {
            if let Some(gt) = after_cap[p_start..].find('>') {
                let content_start = p_start + gt + 1;
                if let Some(p_end) = after_cap[content_start..].find("</p>") {
                    let raw = &after_cap[content_start..content_start + p_end];
                    let text = strip_html_tags(raw);
                    let text = decode_html_entities(&text).trim().to_string();
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
        }
    }

    // Fallback: find any <p> tag with meaningful content
    let mut pos = 0;
    while let Some(p_start) = html[pos..].find("<p") {
        let abs = pos + p_start;
        if let Some(gt) = html[abs..].find('>') {
            let content_start = abs + gt + 1;
            if let Some(p_end) = html[content_start..].find("</p>") {
                let raw = &html[content_start..content_start + p_end];
                let text = strip_html_tags(raw);
                let text = decode_html_entities(&text).trim().to_string();
                if text.len() > 20 {
                    return text;
                }
                pos = content_start + p_end + 4;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    String::new()
}

/// Strip HTML tags from a string.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Decode common HTML entities.
fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#0183;", "·")
        .replace("&ensp;", " ")
        .replace("&#227;", "ã")
        .replace("&#233;", "é")
        .replace("&#234;", "ê")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<b>hello</b> world"), "hello world");
        assert_eq!(strip_html_tags("no tags here"), "no tags here");
        assert_eq!(strip_html_tags("<a href=\"x\">link</a>"), "link");
    }

    #[test]
    fn test_decode_html_entities() {
        assert_eq!(decode_html_entities("a &amp; b"), "a & b");
        assert_eq!(decode_html_entities("&lt;tag&gt;"), "<tag>");
    }

    #[test]
    fn test_parse_bing_html_basic() {
        let html = r#"
        <div>some header</div>
        <li class="b_algo" data-id iid=SERP.1>
            <h2><a href="https://example.com/page1" h="ID=SERP">Example Page Title</a></h2>
            <div class="b_caption">
                <p>This is the snippet for the first result with enough content to be useful.</p>
            </div>
        </li>
        <li class="b_algo" data-id iid=SERP.2>
            <h2><a href="https://test.org/page2" h="ID=SERP">Test Page</a></h2>
            <div class="b_caption">
                <p>Another snippet with some informative text about the search result.</p>
            </div>
        </li>
        "#;

        let results = parse_bing_html(html, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Page Title");
        assert_eq!(results[0].url, "https://example.com/page1");
        assert!(results[0].content.contains("snippet for the first result"));
        assert_eq!(results[1].title, "Test Page");
        assert_eq!(results[1].url, "https://test.org/page2");
    }

    #[test]
    fn test_parse_bing_html_empty() {
        let html = "<html><body>No results</body></html>";
        let results = parse_bing_html(html, 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_bing_html_modern_structure() {
        // Real Bing CN HTML structure with <h2 class=""> and favicon links
        let html = r#"
        <div>header</div>
        <li class="b_algo" data-id iid=SERP.5344>
            <div class="b_tpcn">
                <a class="tilk" aria-label="example.com" href="https://example.com/page" h="ID=SERP,1.1">
                    <div class="tptxt"><div class="tptt">example.com</div></div>
                </a>
            </div>
            <h2 class="">
                <a target="_blank" href="https://example.com/page" h="ID=SERP,1.2">持续关注<strong>伊朗局势</strong>_央广网</a>
            </h2>
            <div class="b_caption">
                <p class="b_lineclamp2">22 小时之前&ensp;&#0183;&ensp;伊朗伊斯兰革命卫队发布公告内容</p>
            </div>
        </li>
        "#;

        let results = parse_bing_html(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "持续关注伊朗局势_央广网");
        assert_eq!(results[0].url, "https://example.com/page");
        assert!(results[0].content.contains("伊朗伊斯兰革命卫队"));
    }

    #[test]
    fn test_parse_bing_html_max_results() {
        let html = r#"
        <li class="b_algo"><h2><a href="https://a.com">A</a></h2><div class="b_caption"><p>Snippet A is long enough to pass.</p></div></li>
        <li class="b_algo"><h2><a href="https://b.com">B</a></h2><div class="b_caption"><p>Snippet B is long enough to pass.</p></div></li>
        <li class="b_algo"><h2><a href="https://c.com">C</a></h2><div class="b_caption"><p>Snippet C is long enough to pass.</p></div></li>
        "#;
        let results = parse_bing_html(html, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_new_client() {
        let _client = BingClient::new();
    }

    #[test]
    fn test_extract_first_url_skips_bing() {
        let html = r#"<a href="https://www.bing.com/foo">x</a><a href="https://example.com/bar">y</a>"#;
        assert_eq!(extract_first_url(html), Some("https://example.com/bar".to_string()));
    }
}
