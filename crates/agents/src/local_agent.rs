use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};
use tokenmind_core::{task::Task, context::AppContext};
use crate::Agent;

pub struct LocalAgent {
    confidence_threshold: f32,
}

impl LocalAgent {
    pub fn new(confidence_threshold: f32) -> Self {
        Self { confidence_threshold }
    }

    /// Download + load the model if not already in memory.
    /// Runs in a blocking thread to avoid blocking the async runtime.
    pub async fn ensure_ready() -> Result<()> {
        if local_llm::is_model_loaded() {
            return Ok(());
        }
        info!("[LocalAgent] Modèle non chargé — téléchargement + chargement en cours…");
        tokio::task::spawn_blocking(|| {
            local_llm::ensure_model_loaded()
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await??;
        info!("[LocalAgent] Modèle llama-cpp-4 prêt");
        Ok(())
    }

    pub fn is_loaded() -> bool {
        local_llm::is_model_loaded()
    }

    fn estimate_confidence(response: &str) -> f32 {
        if response.is_empty() {
            return 0.0;
        }
        let refusal = ["I cannot", "I don't know", "Erreur", "non chargé", "inexploitable"];
        if refusal.iter().any(|p| response.contains(p)) {
            return 0.3;
        }
        let len = response.len();
        if len < 10 { 0.5 } else if len < 50 { 0.72 } else { 0.92 }
    }
}

#[async_trait]
impl Agent for LocalAgent {
    fn name(&self) -> &str { "LocalAgent (qwen2.5-coder — llama-cpp-4)" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[LocalAgent] Tâche triviale : {}", task.description);

        // Ensure model is loaded (downloads from HuggingFace if needed)
        Self::ensure_ready().await?;

        let file_content = task.file_targets.iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .collect::<Vec<_>>()
            .join("\n");

        let description = task.description.clone();
        let snippet = if file_content.is_empty() {
            String::new()
        } else {
            file_content[..file_content.len().min(2000)].to_string()
        };

        // Run blocking inference on a dedicated thread
        let response = tokio::task::spawn_blocking(move || {
            if snippet.is_empty() {
                local_llm::run_llm_request(description)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            } else {
                let rules = format!(
                    "You are a code assistant. Handle trivial code tasks concisely.\n\
                     File content:\n{}",
                    snippet
                );
                tokio::runtime::Handle::current()
                    .block_on(local_llm::run_llm_request_rag(description, rules))
                    .map_err(|e| anyhow::anyhow!("{}", e))
            }
        })
        .await??;

        let confidence = Self::estimate_confidence(&response);
        info!("[LocalAgent] Réponse {:.2} confiance ({} chars)", confidence, response.len());

        if confidence < self.confidence_threshold {
            warn!(
                "[LocalAgent] Confiance {:.2} < seuil {:.2} — escalade vers LLM cloud",
                confidence, self.confidence_threshold
            );
            anyhow::bail!("Local LLM confidence too low ({:.2}), escalating", confidence);
        }

        task.mark_done(response.clone(), 0);
        Ok(response)
    }
}
