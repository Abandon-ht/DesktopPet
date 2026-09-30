//! Bounded, read-only web retrieval for the optional agent.

use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, Url, header};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const KEYCHAIN_SERVICE: &str = "dev.desktoppet.web-search.brave";
const KEYCHAIN_ACCOUNT: &str = "default";
const MAX_BYTES: usize = 2 * 1024 * 1024;

mod weather;
pub use weather::{WeatherDay, WeatherReport, lookup_weather};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchProvider {
    #[default]
    Wikipedia,
    Brave,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub retrieved_at: u64,
}

#[derive(Clone, Debug)]
pub struct Page {
    pub title: String,
    pub url: String,
    pub text: String,
    pub retrieved_at: u64,
}

pub struct WebRetriever {
    provider: SearchProvider,
    brave_key: Option<String>,
}

impl WebRetriever {
    pub fn new(provider: SearchProvider, brave_key: Option<String>) -> Result<Self> {
        Ok(Self {
            provider,
            brave_key,
        })
    }

    pub fn provider(&self) -> SearchProvider {
        self.provider
    }

    /// Use the saved key for time-sensitive questions while keeping Wikipedia
    /// as the default source for ordinary reference questions.
    pub fn brave_for_current_query(&self) -> Option<Self> {
        self.brave_key.as_ref().map(|key| Self {
            provider: SearchProvider::Brave,
            brave_key: Some(key.clone()),
        })
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Source>> {
        let query = query.trim();
        ensure!(
            !query.is_empty() && query.chars().count() <= 300,
            "查询词须为 1–300 字"
        );
        let now = epoch_seconds();
        match self.provider {
            SearchProvider::Wikipedia => {
                let language = if query
                    .chars()
                    .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
                {
                    "zh"
                } else {
                    "en"
                };
                let mut url = Url::parse(&format!("https://{language}.wikipedia.org/w/api.php"))?;
                url.query_pairs_mut()
                    .append_pair("action", "query")
                    .append_pair("list", "search")
                    .append_pair("srsearch", query)
                    .append_pair("format", "json")
                    .append_pair("srlimit", "5");
                let (body, _) = safe_get(url, None).await?;
                let response: WikipediaResponse =
                    serde_json::from_slice(&body).context("维基百科搜索响应无效")?;
                Ok(response
                    .query
                    .search
                    .into_iter()
                    .take(5)
                    .enumerate()
                    .map(|(index, item)| Source {
                        id: format!("S{}", index + 1),
                        title: item.title,
                        url: format!("https://{language}.wikipedia.org/?curid={}", item.pageid),
                        snippet: html_text(&item.snippet, 600),
                        retrieved_at: now,
                    })
                    .collect())
            }
            SearchProvider::Brave => {
                let mut url = Url::parse("https://api.search.brave.com/res/v1/web/search")?;
                url.query_pairs_mut()
                    .append_pair("q", query)
                    .append_pair("count", "5");
                let token = self
                    .brave_key
                    .as_deref()
                    .context("Brave Search API key 未设置")?;
                let (body, _) = safe_get(url, Some(token)).await?;
                let response: BraveResponse =
                    serde_json::from_slice(&body).context("Brave 搜索响应无效")?;
                Ok(response
                    .web
                    .results
                    .into_iter()
                    .take(5)
                    .enumerate()
                    .filter_map(|(index, item)| {
                        let url = Url::parse(&item.url).ok()?;
                        if !matches!(url.scheme(), "http" | "https")
                            || !url.username().is_empty()
                            || url.password().is_some()
                            || !url.host_str().is_some_and(external_hostname)
                        {
                            return None;
                        }
                        Some(Source {
                            id: format!("S{}", index + 1),
                            title: item.title,
                            url: item.url,
                            snippet: html_text(&item.description, 600),
                            retrieved_at: now,
                        })
                    })
                    .collect())
            }
        }
    }

    pub async fn fetch_page(&self, url: &str) -> Result<Page> {
        let url = Url::parse(url).context("网页 URL 无效")?;
        let (body, final_url) = safe_get(url, None).await?;
        let html = String::from_utf8_lossy(&body);
        let document = Html::parse_document(&html);
        let title_selector = Selector::parse("title").expect("static selector");
        let paragraph_selector =
            Selector::parse("article p, main p, body p").expect("static selector");
        let title = document
            .select(&title_selector)
            .next()
            .map(|node| normalize(&node.text().collect::<Vec<_>>().join(" "), 200))
            .unwrap_or_default();
        let text = document
            .select(&paragraph_selector)
            .take(80)
            .map(|node| normalize(&node.text().collect::<Vec<_>>().join(" "), 1000))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        ensure!(!text.is_empty(), "网页没有可提取的正文");
        Ok(Page {
            title,
            url: final_url.to_string(),
            text: normalize(&text, 8000),
            retrieved_at: epoch_seconds(),
        })
    }
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalize(text: &str, max_chars: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn html_text(html: &str, max_chars: usize) -> String {
    let fragment = Html::parse_fragment(html);
    normalize(
        &fragment.root_element().text().collect::<Vec<_>>().join(" "),
        max_chars,
    )
}

async fn safe_get(mut url: Url, brave_key: Option<&str>) -> Result<(Vec<u8>, Url)> {
    ensure!(url.as_str().len() <= 2048, "网页 URL 过长");
    let initial_host = url.host_str().context("网页 URL 缺少域名")?.to_string();
    for _ in 0..=3 {
        ensure!(
            matches!(url.scheme(), "http" | "https"),
            "只支持 HTTP(S) 网页"
        );
        if brave_key.is_some() {
            ensure!(url.scheme() == "https", "搜索密钥只允许通过 HTTPS 发送");
        }
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "网页 URL 不可含用户凭据"
        );
        let host = url.host_str().context("网页 URL 缺少域名")?.to_string();
        ensure!(external_hostname(&host), "网页主机名不受支持");
        if brave_key.is_some() && host != initial_host {
            bail!("搜索服务不能跨站重定向密钥");
        }
        let port = url.port_or_known_default().context("网页 URL 缺少端口")?;
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .context("网页地址解析失败")?
            .collect();
        ensure!(
            !addresses.is_empty()
                && addresses
                    .iter()
                    .all(|addr| acceptable_address(addr.ip(), &host)),
            "网页地址指向受保护网络"
        );
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .resolve_to_addrs(&host, &addresses)
            .user_agent("DesktopPet/0.1 (read-only search)")
            .build()?;
        let mut request = client
            .get(url.clone())
            .header(header::ACCEPT, "text/html, application/json");
        if let Some(key) = brave_key {
            request = request.header("X-Subscription-Token", key);
        }
        let response = request.send().await.context("网页请求失败")?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .context("网页重定向缺少目标")?
                .to_str()?;
            url = url.join(location)?;
            continue;
        }
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            bail!("搜索服务限流，请稍后重试");
        }
        let response = response.error_for_status().context("网页服务返回错误")?;
        let media = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        ensure!(
            media.starts_with("text/html")
                || media.starts_with("application/json")
                || media.starts_with("text/plain"),
            "网页内容类型不受支持"
        );
        if let Some(size) = response.content_length() {
            ensure!(size <= MAX_BYTES as u64, "网页响应过大");
        }
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("读取网页失败")?;
            ensure!(
                body.len().saturating_add(chunk.len()) <= MAX_BYTES,
                "网页响应过大"
            );
            body.extend_from_slice(&chunk);
        }
        return Ok((body, url));
    }
    bail!("网页重定向次数过多")
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_v4(mapped);
            }
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && !ip.segments().starts_with(&[0x2001, 0x0db8])
        }
    }
}

fn external_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host.contains('.')
        && !host.ends_with(".local")
        && !host.ends_with(".localhost")
        && !host.ends_with(".internal")
        && !host.ends_with(".test")
        && !host.ends_with(".invalid")
        && host != "localhost"
        && host.parse::<IpAddr>().is_err()
}

fn acceptable_address(ip: IpAddr, host: &str) -> bool {
    if public_ip(ip) {
        return true;
    }
    // Some macOS network extensions use these benchmark ranges as synthetic
    // DNS answers for public domain names. Literal IP URLs remain forbidden.
    let synthetic_proxy = match ip {
        IpAddr::V4(ip) => matches!(ip.octets(), [198, 18 | 19, _, _]),
        IpAddr::V6(ip) => ip.segments()[..3] == [0x2001, 0x0002, 0],
    };
    synthetic_proxy && external_hostname(host)
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && (b == 0 || b == 168))
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113))
}

#[derive(Deserialize)]
struct WikipediaResponse {
    query: WikipediaQuery,
}
#[derive(Deserialize)]
struct WikipediaQuery {
    search: Vec<WikipediaItem>,
}
#[derive(Deserialize)]
struct WikipediaItem {
    title: String,
    pageid: u64,
    snippet: String,
}
#[derive(Deserialize)]
struct BraveResponse {
    web: BraveWeb,
}
#[derive(Deserialize)]
struct BraveWeb {
    results: Vec<BraveItem>,
}
#[derive(Deserialize)]
struct BraveItem {
    title: String,
    url: String,
    description: String,
}

#[cfg(target_os = "macos")]
pub fn save_brave_key(key: &str) -> Result<()> {
    ensure!(!key.trim().is_empty() && key.len() <= 512, "API key 无效");
    security_framework::passwords::set_generic_password(
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT,
        key.as_bytes(),
    )?;
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn save_brave_key(_: &str) -> Result<()> {
    bail!("当前平台未接入系统凭据库")
}

#[cfg(target_os = "macos")]
pub fn load_brave_key() -> Option<String> {
    security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .ok()
        .and_then(|value| String::from_utf8(value).ok())
}
#[cfg(not(target_os = "macos"))]
pub fn load_brave_key() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
pub fn delete_brave_key() -> Result<()> {
    security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?;
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn delete_brave_key() -> Result<()> {
    bail!("当前平台未接入系统凭据库")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_private_and_reserved_targets() {
        for text in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "169.254.1.2",
            "::1",
            "fc00::1",
            "2001:db8::1",
        ] {
            assert!(!public_ip(text.parse().unwrap()), "{text}");
        }
        assert!(public_ip("1.1.1.1".parse().unwrap()));
        assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
        assert!(!acceptable_address(
            "198.18.0.64".parse().unwrap(),
            "localhost"
        ));
        assert!(acceptable_address(
            "198.18.0.64".parse().unwrap(),
            "en.wikipedia.org"
        ));
    }
    #[test]
    fn strips_html_from_search_snippet() {
        assert_eq!(
            html_text("<span>Rust</span> <b>language</b>", 100),
            "Rust language"
        );
    }
    #[tokio::test]
    #[ignore = "requires public internet"]
    async fn wikipedia_live_search() {
        let web = WebRetriever::new(SearchProvider::Wikipedia, None).unwrap();
        let sources = web.search("Rust programming language").await.unwrap();
        assert!(!sources.is_empty());
        assert!(
            sources
                .iter()
                .all(|source| source.url.starts_with("https://en.wikipedia.org/"))
        );
        let page = web.fetch_page(&sources[0].url).await.unwrap();
        assert!(!page.text.is_empty());
        let chinese = web.search("中国").await.unwrap();
        assert!(!chinese.is_empty());
        assert!(chinese[0].url.starts_with("https://zh.wikipedia.org/"));
    }
}
