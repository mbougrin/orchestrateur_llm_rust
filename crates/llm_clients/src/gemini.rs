use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::TokenUsage;

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
