//! HTTP inference adapters. Model and endpoint selection belong to the caller.

use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LmStudioConfig {
    /// Accepts either the server root or its `/v1` compatibility base URL.
    pub base_url: String,
    pub model_id: String,
    pub api_key: Option<String>,
    pub max_output_tokens: u32,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl LmStudioConfig {
    pub fn new(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model_id: model_id.into(),
            api_key: None,
            max_output_tokens: 512,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(180),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

pub struct LmStudioBackend {
    client: Client,
    base_url: Url,
    config: LmStudioConfig,
}

#[derive(Serialize)]
struct NativeChatRequest<'a> {
    model: &'a str,
    input: &'a str,
    system_prompt: &'a str,
    reasoning: &'static str,
    max_output_tokens: u32,
    stream: bool,
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: Option<&'a str>,
}

pub struct NativeReply {
    pub text: String,
    pub response_id: Option<String>,
}

impl LmStudioBackend {
    pub fn new(config: LmStudioConfig) -> Result<Self> {
        if config.model_id.trim().is_empty() || config.max_output_tokens == 0 {
            bail!("model ID and max output tokens must be set");
        }
        let mut base_url = Url::parse(&config.base_url).context("invalid LM Studio URL")?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || !matches!(base_url.path(), "/" | "" | "/v1" | "/v1/")
        {
            bail!("LM Studio URL must be an HTTP(S) server root or /v1 base URL");
        }
        base_url.set_path("/v1/");
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = config.api_key.as_deref().filter(|key| !key.is_empty()) {
            let bearer = format!("Bearer {key}");
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&bearer)
                    .context("invalid API key header")?,
            );
        }
        let client = Client::builder()
            .default_headers(headers)
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .build()?;
        Ok(Self {
            client,
            base_url,
            config,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.config.model_id
    }

    /// Discovery only: a listed model may still need loading or warmup.
    pub async fn probe(&self) -> Result<()> {
        let url = self.base_url.join("models")?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("LM Studio is unreachable")?;
        let response = response
            .error_for_status()
            .context("LM Studio model list failed")?;
        let models: ModelList = response
            .json()
            .await
            .context("invalid LM Studio model list")?;
        if !models.data.iter().any(|m| m.id == self.config.model_id) {
            bail!("LM Studio does not list model {}", self.config.model_id);
        }
        Ok(())
    }

    /// Emits only user-visible `delta.content`; reasoning and tool fields are ignored.
    /// Dropping this future closes the HTTP stream, allowing caller-driven cancellation.
    pub async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        mut on_text: impl FnMut(&str),
    ) -> Result<String> {
        if messages.is_empty() || messages.iter().any(|m| m.content.trim().is_empty()) {
            bail!("chat messages must contain non-empty content");
        }
        if messages
            .iter()
            .any(|m| !matches!(m.role.as_str(), "system" | "user" | "assistant"))
        {
            bail!("unsupported chat message role");
        }
        let url = self.base_url.join("chat/completions")?;
        let response = self
            .client
            .post(url)
            .header("Accept", "text/event-stream")
            .json(&ChatRequest {
                model: &self.config.model_id,
                messages,
                max_tokens: self.config.max_output_tokens,
                stream: true,
            })
            .send()
            .await
            .context("LM Studio chat request failed")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!(
                "LM Studio HTTP {status}: {}",
                body.chars().take(500).collect::<String>()
            );
        }
        let mut bytes = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut answer = String::new();
        let mut done = false;
        while let Some(chunk) = bytes.next().await {
            for data in decoder.push(&chunk.context("LM Studio stream disconnected")?)? {
                if data == "[DONE]" {
                    done = true;
                    break;
                }
                let value: serde_json::Value =
                    serde_json::from_str(&data).context("invalid LM Studio stream event")?;
                if let Some(error) = value.get("error") {
                    bail!("LM Studio stream error: {error}");
                }
                if let Some(choice) = value.get("choices").and_then(|c| c.get(0)) {
                    if choice.get("finish_reason").and_then(|v| v.as_str()) == Some("length") {
                        bail!("LM Studio reply reached the output token limit");
                    }
                    if let Some(delta) = choice
                        .get("delta")
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        if !delta.is_empty() {
                            answer.push_str(delta);
                            on_text(delta);
                        }
                    }
                }
            }
            if done {
                break;
            }
        }
        if !done {
            bail!("LM Studio stream ended without [DONE]");
        }
        if answer.trim().is_empty() {
            bail!("LM Studio returned no spoken answer text");
        }
        Ok(answer)
    }

    /// LM Studio's native API permits per-request reasoning control. For the
    /// configured Qwen model this is needed to obtain a timely spoken answer.
    /// Stateless convenience method for probes and single-turn clients.
    pub async fn stream_native_chat(
        &self,
        input: &str,
        system_prompt: &str,
        on_text: impl FnMut(&str),
    ) -> Result<String> {
        self.stream_native_chat_with_context(input, system_prompt, None, false, on_text)
            .await
            .map(|reply| reply.text)
    }

    /// `previous_response_id` is scoped to one conversation by the caller.
    /// `store=false` leaves no continuation ID and keeps turns stateless.
    pub async fn stream_native_chat_with_context(
        &self,
        input: &str,
        system_prompt: &str,
        previous_response_id: Option<&str>,
        store: bool,
        mut on_text: impl FnMut(&str),
    ) -> Result<NativeReply> {
        if input.trim().is_empty() {
            bail!("chat input must not be empty");
        }
        let url = self.base_url.join("../api/v1/chat")?;
        let response = self
            .client
            .post(url)
            .header("Accept", "text/event-stream")
            .json(&NativeChatRequest {
                model: &self.config.model_id,
                input,
                system_prompt,
                reasoning: "off",
                max_output_tokens: self.config.max_output_tokens,
                stream: true,
                store,
                previous_response_id,
            })
            .send()
            .await
            .context("LM Studio native chat request failed")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!(
                "LM Studio HTTP {status}: {}",
                body.chars().take(500).collect::<String>()
            );
        }
        let mut bytes = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut answer = String::new();
        let mut response_id = None;
        let mut ended = false;
        while let Some(chunk) = bytes.next().await {
            for data in decoder.push(&chunk.context("LM Studio native stream disconnected")?)? {
                let value: serde_json::Value =
                    serde_json::from_str(&data).context("invalid LM Studio native stream event")?;
                match value.get("type").and_then(|v| v.as_str()) {
                    Some("message.delta") => {
                        if let Some(delta) = value.get("content").and_then(|v| v.as_str()) {
                            answer.push_str(delta);
                            on_text(delta);
                        }
                    }
                    Some("chat.end") => {
                        ended = true;
                        response_id = value
                            .get("result")
                            .and_then(|r| r.get("response_id"))
                            .and_then(|v| v.as_str())
                            .map(str::to_owned);
                        if let Some(final_text) = value
                            .get("result")
                            .and_then(|r| r.get("output"))
                            .and_then(|o| o.as_array())
                            .and_then(|o| {
                                o.iter().find(|entry| {
                                    entry.get("type").and_then(|t| t.as_str()) == Some("message")
                                })
                            })
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_str())
                        {
                            answer = final_text.to_owned();
                        }
                    }
                    Some("error") => bail!("LM Studio native stream error: {value}"),
                    _ => {} // Model-load, reasoning and tool events are never spoken.
                }
            }
            if ended {
                break;
            }
        }
        if !ended {
            bail!("LM Studio native stream ended without chat.end");
        }
        if answer.trim().is_empty() {
            bail!("LM Studio returned no spoken answer text");
        }
        Ok(NativeReply {
            text: answer,
            response_id,
        })
    }
}

#[derive(Deserialize)]
struct ModelList {
    data: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: u32,
    stream: bool,
}

#[derive(Default)]
struct SseDecoder {
    line: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseDecoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        let mut events = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if self.line.last() == Some(&b'\r') {
                    self.line.pop();
                }
                let line = std::str::from_utf8(&self.line).context("non-UTF8 SSE line")?;
                if line.is_empty() {
                    if !self.data_lines.is_empty() {
                        events.push(self.data_lines.join("\n"));
                        self.data_lines.clear();
                    }
                } else if let Some(data) = line.strip_prefix("data:") {
                    self.data_lines
                        .push(data.strip_prefix(' ').unwrap_or(data).to_owned());
                }
                self.line.clear();
            } else {
                self.line.push(byte);
                if self.line.len() > 128 * 1024 {
                    return Err(anyhow!("SSE line exceeds 128 KiB"));
                }
            }
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_root_and_v1_urls() {
        for base in ["http://127.0.0.1:1234", "http://127.0.0.1:1234/v1/"] {
            let backend = LmStudioBackend::new(LmStudioConfig::new(base, "qwen/example")).unwrap();
            assert_eq!(backend.base_url.as_str(), "http://127.0.0.1:1234/v1/");
        }
        assert!(LmStudioBackend::new(LmStudioConfig::new("file:///tmp", "x")).is_err());
    }

    #[test]
    fn decodes_split_crlf_events() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push(b"data: {\"choices\":").unwrap().is_empty());
        let events = decoder.push(b"[]}\r\n\r\ndata: [DONE]\n\n").unwrap();
        assert_eq!(events, ["{\"choices\":[]}", "[DONE]"]);
    }
}
