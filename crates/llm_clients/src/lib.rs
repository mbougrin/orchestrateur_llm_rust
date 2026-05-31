pub mod anthropic;
pub mod gemini;
pub mod grok;
pub mod gpt;

/// A chunk event emitted by a streaming LLM call.
#[derive(Debug)]
pub enum StreamEvent {
    Chunk(String),
    Done(TokenUsage),
    Error(String),
}

/// Shared type: agents send `(task_id, chunk)` pairs here; the TUI relays them.
pub type StreamSink = std::sync::Arc<tokio::sync::mpsc::UnboundedSender<(uuid::Uuid, String)>>;

/// Convenience: accumulate a stream receiver into `(full_text, usage)`.
pub async fn collect_stream(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<StreamEvent>,
    sink: Option<&StreamSink>,
    task_id: uuid::Uuid,
) -> anyhow::Result<(String, TokenUsage)> {
    let mut text = String::new();
    let mut usage = TokenUsage::default();
    while let Some(ev) = rx.recv().await {
        match ev {
            StreamEvent::Chunk(chunk) => {
                if let Some(s) = sink {
                    let _ = s.send((task_id, chunk.clone()));
                }
                text.push_str(&chunk);
            }
            StreamEvent::Done(u) => { usage = u; }
            StreamEvent::Error(e) => anyhow::bail!(e),
        }
    }
    Ok((text, usage))
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl TokenUsage {
    pub fn new(input: u32, output: u32) -> Self {
        Self { input_tokens: input, output_tokens: output }
    }

    pub fn cost_usd(&self, model: &LlmModel) -> f64 {
        let pricing = TokenPricing::default();
        match model {
            LlmModel::ClaudeSonnet => {
                (self.input_tokens as f64 / 1_000_000.0) * pricing.claude_sonnet_in
                    + (self.output_tokens as f64 / 1_000_000.0) * pricing.claude_sonnet_out
            }
            LlmModel::ClaudeHaiku => {
                (self.input_tokens as f64 / 1_000_000.0) * pricing.claude_haiku_in
                    + (self.output_tokens as f64 / 1_000_000.0) * pricing.claude_haiku_out
            }
            LlmModel::Gemini => {
                (self.input_tokens as f64 / 1_000_000.0) * pricing.gemini_flash_in
                    + (self.output_tokens as f64 / 1_000_000.0) * pricing.gemini_flash_out
            }
            LlmModel::Grok => {
                (self.input_tokens as f64 / 1_000_000.0) * pricing.grok_mini_in
                    + (self.output_tokens as f64 / 1_000_000.0) * pricing.grok_mini_out
            }
            LlmModel::Gpt => {
                (self.input_tokens as f64 / 1_000_000.0) * pricing.gpt_4o_mini_in
                    + (self.output_tokens as f64 / 1_000_000.0) * pricing.gpt_4o_mini_out
            }
            LlmModel::Local => 0.0,
        }
    }
}

impl std::ops::AddAssign for TokenUsage {
    fn add_assign(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum LlmModel {
    ClaudeSonnet,
    ClaudeHaiku,
    Gemini,
    Grok,
    Gpt,
    Local,
}

impl LlmModel {
    pub fn display_name(&self) -> &str {
        match self {
            LlmModel::ClaudeSonnet => "claude-sonnet-4-5",
            LlmModel::ClaudeHaiku  => "claude-haiku-4-5",
            LlmModel::Gemini       => "gemini-2.0-flash",
            LlmModel::Grok         => "grok-3-mini",
            LlmModel::Gpt          => "gpt-4o-mini",
            LlmModel::Local        => "qwen2.5-coder:3b",
        }
    }

    pub fn api_model_id(&self) -> &str {
        match self {
            LlmModel::ClaudeSonnet => "claude-sonnet-4-5",
            LlmModel::ClaudeHaiku  => "claude-haiku-4-5",
            LlmModel::Gemini       => "gemini-2.0-flash",
            LlmModel::Grok         => "grok-3-mini",
            LlmModel::Gpt          => "gpt-4o-mini",
            LlmModel::Local        => "local",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: MessageRole::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: MessageRole::Assistant, content: content.into() }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: MessageRole::System, content: content.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// Heuristic token estimator: ~4 chars ≈ 1 BPE token for code/English.
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Estimated input cost for `tokens` tokens on `model` (USD).
pub fn estimate_cost_usd(tokens: usize, model: &LlmModel) -> f64 {
    let p = TokenPricing::default();
    let t = tokens as f64 / 1_000_000.0;
    match model {
        LlmModel::ClaudeSonnet => t * p.claude_sonnet_in,
        LlmModel::ClaudeHaiku  => t * p.claude_haiku_in,
        LlmModel::Gemini       => t * p.gemini_flash_in,
        LlmModel::Grok         => t * p.grok_mini_in,
        LlmModel::Gpt          => t * p.gpt_4o_mini_in,
        LlmModel::Local        => 0.0,
    }
}

pub struct TokenPricing {
    pub claude_sonnet_in: f64,
    pub claude_sonnet_out: f64,
    pub claude_haiku_in: f64,
    pub claude_haiku_out: f64,
    pub gemini_flash_in: f64,
    pub gemini_flash_out: f64,
    pub grok_mini_in: f64,
    pub grok_mini_out: f64,
    pub gpt_4o_mini_in: f64,
    pub gpt_4o_mini_out: f64,
    pub local: f64,
}

impl Default for TokenPricing {
    fn default() -> Self {
        Self {
            claude_sonnet_in: 3.00,
            claude_sonnet_out: 15.00,
            claude_haiku_in: 0.25,
            claude_haiku_out: 1.25,
            gemini_flash_in: 0.075,
            gemini_flash_out: 0.30,
            grok_mini_in: 0.30,
            grok_mini_out: 0.50,
            gpt_4o_mini_in: 0.15,
            gpt_4o_mini_out: 0.60,
            local: 0.0,
        }
    }
}
