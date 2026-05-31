use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::TokenUsage;

const GPT_API_BASE: &str = "https://api.openai.com/v1/chat/completions";
const GPT_MODEL: &str = "gpt-4o-mini";

pub struct GptClient {
    api_key: String,
    http: Client,
    pub token_counter: Arc<Mutex<TokenUsage>>,
}

impl GptClient {
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
        let mut messages: Vec<GptMessage> = Vec::new();
        if !system.is_empty() {
            messages.push(GptMessage { role: "system".to_string(), content: system.to_string() });
        }
        messages.push(GptMessage { role: "user".to_string(), content: user_prompt.to_string() });

        let request_body = GptRequest {
            model: GPT_MODEL.to_string(),
            messages,
            max_tokens,
        };

        let response = self
            .http
            .post(GPT_API_BASE)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status == 429 {
                anyhow::bail!("rate_limit: GPT quota exceeded (429) — {}", body);
            }
            anyhow::bail!("GPT API error {}: {}", status, body);
        }

        let api_response: GptResponse = response.json().await?;

        let content = api_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        let usage = TokenUsage::new(
            api_response.usage.prompt_tokens,
            api_response.usage.completion_tokens,
        );

        if let Ok(mut counter) = self.token_counter.lock() {
            *counter += usage.clone();
        }

        Ok((content, usage))
    }
}

#[derive(Serialize)]
struct GptRequest {
    model: String,
    messages: Vec<GptMessage>,
    max_tokens: u32,
}

#[derive(Serialize, Deserialize)]
struct GptMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct GptResponse {
    choices: Vec<GptChoice>,
    usage: GptUsage,
}

#[derive(Deserialize)]
struct GptChoice {
    message: GptMessage,
}

#[derive(Deserialize)]
struct GptUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}
