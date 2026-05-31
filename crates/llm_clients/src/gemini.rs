use anyhow::Result;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::{StreamEvent, TokenUsage};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

pub struct GeminiClient {
    api_key: String,
    http: Client,
    pub token_counter: Arc<Mutex<TokenUsage>>,
}

impl GeminiClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: Client::new(),
            token_counter: Arc::new(Mutex::new(TokenUsage::default())),
        }
    }

    pub async fn complete(
        &self,
        system: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<(String, TokenUsage)> {
        let url = format!(
            "{}/gemini-2.0-flash:generateContent?key={}",
            GEMINI_API_BASE, self.api_key
        );

        let mut contents: Vec<GeminiContent> = Vec::new();

        if !system.is_empty() {
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart { text: format!("System: {}", system) }],
            });
            contents.push(GeminiContent {
                role: "model".to_string(),
                parts: vec![GeminiPart { text: "Understood.".to_string() }],
            });
        }

        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart { text: user_prompt.to_string() }],
        });

        let request_body = GeminiRequest {
            contents,
            generation_config: GenerationConfig { max_output_tokens: max_tokens },
        };

        let response = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status == 429 {
                anyhow::bail!("rate_limit: Gemini quota exceeded (429) — {}", body);
            }
            anyhow::bail!("Gemini API error {}: {}", status, body);
        }

        let api_response: GeminiResponse = response.json().await?;

        let content = api_response
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text)
            .unwrap_or_default();

        let usage = TokenUsage::new(
            api_response.usage_metadata.prompt_token_count,
            api_response.usage_metadata.candidates_token_count,
        );

        if let Ok(mut counter) = self.token_counter.lock() {
            *counter += usage.clone();
        }

        Ok((content, usage))
    }

    /// Streaming variant using Gemini SSE (`streamGenerateContent?alt=sse`).
    pub async fn complete_stream(
        &self,
        system: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

        let url = format!(
            "{}/gemini-2.0-flash:streamGenerateContent?key={}&alt=sse",
            GEMINI_API_BASE, self.api_key
        );

        let mut contents: Vec<GeminiContent> = Vec::new();
        if !system.is_empty() {
            contents.push(GeminiContent { role: "user".to_string(), parts: vec![GeminiPart { text: format!("System: {}", system) }] });
            contents.push(GeminiContent { role: "model".to_string(), parts: vec![GeminiPart { text: "Understood.".to_string() }] });
        }
        contents.push(GeminiContent { role: "user".to_string(), parts: vec![GeminiPart { text: user_prompt.to_string() }] });

        let request_body = GeminiRequest {
            contents,
            generation_config: GenerationConfig { max_output_tokens: max_tokens },
        };

        let response = self.http
            .post(&url)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status == 429 {
                anyhow::bail!("rate_limit: Gemini quota exceeded (429) — {}", body);
            }
            anyhow::bail!("Gemini API error {}: {}", status, body);
        }

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            let mut buf = String::new();
            let mut prompt_tokens = 0u32;
            let mut candidate_tokens = 0u32;

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

                    let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };

                    // Each Gemini SSE chunk is a full GeminiResponse fragment
                    if let Some(text) = v.pointer("/candidates/0/content/parts/0/text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            let _ = tx.send(StreamEvent::Chunk(text.to_string()));
                        }
                    }
                    if let Some(pt) = v.pointer("/usageMetadata/promptTokenCount").and_then(|x| x.as_u64()) {
                        prompt_tokens = pt as u32;
                    }
                    if let Some(ct) = v.pointer("/usageMetadata/candidatesTokenCount").and_then(|x| x.as_u64()) {
                        candidate_tokens = ct as u32;
                    }
                }
            }
            let _ = tx.send(StreamEvent::Done(TokenUsage::new(prompt_tokens, candidate_tokens)));
        });

        Ok(rx)
    }
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: GeminiUsage,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

#[derive(Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
}
