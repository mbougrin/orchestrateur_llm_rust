use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use llm_clients::gemini::GeminiClient;
use llm_clients::LlmModel;
use tokenmind_core::{task::Task, context::AppContext};
use crate::Agent;

pub struct DocAgent;

#[async_trait]
impl Agent for DocAgent {
    fn name(&self) -> &str { "DocAgent" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[DocAgent] Executing: {}", task.description);

        let client = GeminiClient::new(ctx.gemini_key.clone());

        let file_context = build_doc_context(task);

        let system = "You are a technical writer. Write clear, accurate documentation in Markdown. \
            Keep it concise but complete. For README updates, maintain the existing style.";

        let user_prompt = format!(
            "Task: {}\n\nExisting content:\n{}\n\nProvide the updated content.",
            task.description, file_context
        );

        let (response, usage) = client
            .complete(system, &user_prompt, 8192)
            .await?;

        ctx.record_usage(&LlmModel::Gemini, &usage);
        let tokens = usage.input_tokens + usage.output_tokens;
        task.mark_done(response.clone(), tokens);

        info!("[DocAgent] Done. Tokens used: {}", tokens);
        Ok(response)
    }
}

fn build_doc_context(task: &Task) -> String {
    let mut context = String::new();
    for path in &task.file_targets {
        if let Ok(content) = std::fs::read_to_string(path) {
            context.push_str(&format!("--- {}\n{}\n", path.display(), content));
        }
    }
    context
}
