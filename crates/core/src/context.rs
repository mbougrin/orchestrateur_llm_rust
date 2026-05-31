use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use llm_clients::{LlmModel, StreamSink, TokenUsage};
use file_analyzer::GitContext;
use crate::llm_arch_config::LlmArchConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum CostProfile {
    /// Default tier routing (Sonnet/Haiku/Local by priority).
    Balanced,
    /// Force Sonnet for High/Medium tasks, Haiku for Low — highest quality.
    Quality,
    /// Force all tasks to Local — zero API cost.
    Cheap,
}

#[derive(Clone)]
pub struct AppContext {
    pub session_id: String,
    pub project_path: std::path::PathBuf,
    pub token_totals: Arc<Mutex<HashMap<LlmModel, TokenUsage>>>,
    pub cost_total: Arc<Mutex<f64>>,
    pub anthropic_key: String,
    pub gemini_key: String,
    pub grok_key: String,
    pub gpt_key: String,
    pub max_retries: u8,
    pub local_confidence_threshold: f32,
    pub llm_arch: LlmArchConfig,
    /// Set by the TUI to receive streaming chunks. None in non-interactive mode.
    pub stream_sink: Option<StreamSink>,
    /// Content of ORCHESTRATEUR.md / .orchestrateur/instructions.md if found.
    pub project_memory: String,
    /// Refreshed at each planning step.
    pub git: GitContext,
    /// Files added via /add — injected in agent context.
    pub manual_context: Arc<Mutex<Vec<(String, String)>>>,
    /// If true, BuilderAgent writes without threshold check.
    pub auto_write: bool,
    /// When true, agents log full LLM responses to the log panel.
    pub verbose: bool,
    /// Cost/quality routing override.
    pub profile: CostProfile,
}

impl AppContext {
    pub fn new(
        session_id: String,
        project_path: std::path::PathBuf,
        anthropic_key: String,
        gemini_key: String,
        grok_key: String,
        gpt_key: String,
    ) -> Self {
        Self {
            session_id,
            project_path,
            token_totals: Arc::new(Mutex::new(HashMap::new())),
            cost_total: Arc::new(Mutex::new(0.0)),
            anthropic_key,
            gemini_key,
            grok_key,
            gpt_key,
            max_retries: 3,
            local_confidence_threshold: 0.85,
            llm_arch: LlmArchConfig::load(),
            stream_sink: None,
            project_memory: String::new(),
            git: GitContext::default(),
            manual_context: Arc::new(Mutex::new(Vec::new())),
            auto_write: true,
            verbose: false,
            profile: CostProfile::Balanced,
        }
    }

    pub fn record_usage(&self, model: &LlmModel, usage: &TokenUsage) {
        let cost = usage.cost_usd(model);
        if let Ok(mut totals) = self.token_totals.lock() {
            let entry = totals.entry(model.clone()).or_default();
            *entry += usage.clone();
        }
        if let Ok(mut total_cost) = self.cost_total.lock() {
            *total_cost += cost;
        }
    }

    pub fn total_cost(&self) -> f64 {
        *self.cost_total.lock().unwrap()
    }

    pub fn token_count(&self, model: &LlmModel) -> u32 {
        self.token_totals.lock().unwrap()
            .get(model)
            .map(|u| u.input_tokens + u.output_tokens)
            .unwrap_or(0)
    }

    pub fn all_sonnet_cost(&self) -> f64 {
        let totals = self.token_totals.lock().unwrap();
        totals.values().map(|u| {
            let all_sonnet = TokenUsage::new(u.input_tokens, u.output_tokens);
            all_sonnet.cost_usd(&LlmModel::ClaudeSonnet)
        }).sum()
    }

    pub fn savings_percent(&self) -> f64 {
        let all_sonnet = self.all_sonnet_cost();
        let actual = self.total_cost();
        if all_sonnet <= 0.0 { return 0.0; }
        ((all_sonnet - actual) / all_sonnet * 100.0).max(0.0)
    }
}
