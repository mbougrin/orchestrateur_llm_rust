use serde::{Deserialize, Serialize};
use llm_clients::LlmModel;

/// Per-tier LLM routing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierConfig {
    pub preferred: LlmModel,
    pub fallbacks: Vec<LlmModel>,
}

/// Three-tier LLM architecture config.
/// High = Rust/Tauri backend, Medium = Vue/JS/TS/HTML, Low = .md/.json/.sh/.env
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmArchConfig {
    pub high: TierConfig,
    pub medium: TierConfig,
    pub low: TierConfig,
}

impl Default for LlmArchConfig {
    fn default() -> Self {
        Self {
            high: TierConfig {
                preferred: LlmModel::ClaudeSonnet,
                fallbacks: vec![LlmModel::Gemini, LlmModel::Grok],
            },
            medium: TierConfig {
                preferred: LlmModel::ClaudeHaiku,
                fallbacks: vec![LlmModel::Grok, LlmModel::Gemini, LlmModel::Local],
            },
            low: TierConfig {
                preferred: LlmModel::Local,
                fallbacks: vec![LlmModel::Grok],
            },
        }
    }
}

/// Intermediate TOML-serialisable form (model names as strings).
#[derive(Debug, Deserialize)]
struct RawTierConfig {
    preferred: String,
    fallbacks: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawLlmArch {
    high:   Option<RawTierConfig>,
    medium: Option<RawTierConfig>,
    low:    Option<RawTierConfig>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    llm_arch: Option<RawLlmArch>,
}

impl LlmArchConfig {
    /// Load from `~/.config/orchestrateur-llm/config.toml`, falling back to
    /// compiled defaults for any missing section.
    pub fn load() -> Self {
        let path = dirs::home_dir()
            .unwrap_or_default()
            .join(".config/orchestrateur-llm/config.toml");

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };

        let raw: RawConfig = match toml::from_str(&content) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[LlmArchConfig] parse error in config.toml: {}", e);
                return Self::default();
            }
        };

        let defaults = Self::default();
        let arch = match raw.llm_arch {
            Some(a) => a,
            None    => return defaults,
        };

        Self {
            high:   arch.high  .map(|r| parse_tier(r)).unwrap_or(defaults.high),
            medium: arch.medium.map(|r| parse_tier(r)).unwrap_or(defaults.medium),
            low:    arch.low   .map(|r| parse_tier(r)).unwrap_or(defaults.low),
        }
    }
}

fn parse_tier(raw: RawTierConfig) -> TierConfig {
    TierConfig {
        preferred: parse_model(&raw.preferred),
        fallbacks: raw.fallbacks.iter().map(|s| parse_model(s)).collect(),
    }
}

fn parse_model(s: &str) -> LlmModel {
    match s.to_lowercase().as_str() {
        "sonnet" | "claude-sonnet" | "claude_sonnet" => LlmModel::ClaudeSonnet,
        "haiku"  | "claude-haiku"  | "claude_haiku"  => LlmModel::ClaudeHaiku,
        "gemini" | "gemini-flash"                     => LlmModel::Gemini,
        "grok"   | "grok-3-mini"                      => LlmModel::Grok,
        "gpt"    | "gpt-4o-mini"   | "openai"         => LlmModel::Gpt,
        _                                              => LlmModel::Local,
    }
}
