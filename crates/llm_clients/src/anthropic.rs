use anyhow::Result;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::{LlmModel, Message, MessageRole, StreamEvent, TokenUsage};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicClient {
    api_key: String,
    http: Client,
    pub token_counter: Arc<Mutex<TokenUsage>>,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: Client::new(),
            token_counter: Arc::new(Mutex::new(TokenUsage::default())),
        }
    }

    pub async fn complete(
        &self,
        model: &LlmModel,
        system: &str,
        messages: Vec<Message>,
        max_tokens: u32,
    ) -> Result<(String, TokenUsage)> {
        let api_messages: Vec<ApiMessage> = messages
            .iter()
            .filter(|m| !matches!(m.role, MessageRole::System))
            .map(|m| ApiMessage {
                role: match m.role {
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::System => "user".to_string(),
                },
                content: m.content.clone(),
            })
            .collect();

        let request_body = AnthropicRequest {
            model: model.api_model_id().to_string(),
            system: system.to_string(),
            messages: api_messages,
            max_tokens,
        };

        let response = self
            .http
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status == 429 {
                anyhow::bail!("rate_limit: Anthropic quota exceeded (429) — {}", body);
            }
            anyhow::bail!("Anthropic API error {}: {}", status, body);
        }

        let api_response: AnthropicResponse = response.json().await?;

        let content = api_response
            .content
            .into_iter()
            .find(|c| c.content_type == "text")
            .map(|c| c.text)
            .unwrap_or_default();

        let usage = TokenUsage::new(
            api_response.usage.input_tokens,
            api_response.usage.output_tokens,
        );

        if let Ok(mut counter) = self.token_counter.lock() {
            *counter += usage.clone();
        }

        Ok((content, usage))
    }

    /// Streaming variant: returns a channel that emits `Chunk` then `Done`.
    pub async fn complete_stream(
        &self,
        model: &LlmModel,
        system: &str,
        messages: Vec<Message>,
        max_tokens: u32,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

        let api_messages: Vec<ApiMessage> = messages
            .iter()
            .filter(|m| !matches!(m.role, MessageRole::System))
            .map(|m| ApiMessage {
                role: match m.role {
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::System => "user".to_string(),
                },
                content: m.content.clone(),
            })
            .collect();

        let request_body = serde_json::json!({
            "model": model.api_model_id(),
            "system": system,
            "messages": api_messages,
            "max_tokens": max_tokens,
            "stream": true,
        });

        let response = self.http
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status == 429 {
                anyhow::bail!("rate_limit: Anthropic quota exceeded (429) — {}", body);
            }
            anyhow::bail!("Anthropic API error {}: {}", status, body);
        }

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            let mut buf = String::new();
            let mut input_tokens = 0u32;
            let mut output_tokens = 0u32;

            while let Some(chunk_result) = stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => { let _ = tx.send(StreamEvent::Error(e.to_string())); return; }
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));

                loop {
                    let Some(nl) = buf.find('\n') else { break };
                    let line = buf[..nl].trim().to_string();
                    buf = buf[nl + 1..].to_string();

                    let Some(data) = line.strip_prefix("data: ") else { continue };
                    if data == "[DONE]" { break; }

                    let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "message_start" => {
                            if let Some(n) = v.pointer("/message/usage/input_tokens").and_then(|x| x.as_u64()) {
                                input_tokens = n as u32;
                            }
                        }
                        "content_block_delta" => {
                            if v.pointer("/delta/type").and_then(|t| t.as_str()) == Some("text_delta") {
                                if let Some(text) = v.pointer("/delta/text").and_then(|t| t.as_str()) {
                                    let _ = tx.send(StreamEvent::Chunk(text.to_string()));
                                }
                            }
                        }
                        "message_delta" => {
                            if let Some(n) = v.pointer("/usage/output_tokens").and_then(|x| x.as_u64()) {
                                output_tokens = n as u32;
                            }
                        }
                        "message_stop" => {
                            let _ = tx.send(StreamEvent::Done(TokenUsage::new(input_tokens, output_tokens)));
                            return;
                        }
                        _ => {}
                    }
                }
            }
            let _ = tx.send(StreamEvent::Done(TokenUsage::new(input_tokens, output_tokens)));
        });

        Ok(rx)
    }

    pub fn total_cost(&self, model: &LlmModel) -> f64 {
        let counter = self.token_counter.lock().unwrap();
        counter.cost_usd(model)
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    system: String,
    messages: Vec<ApiMessage>,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    usage: UsageBlock,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct UsageBlock {
    input_tokens: u32,
    output_tokens: u32,
}
