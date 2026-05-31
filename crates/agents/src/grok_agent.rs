use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use llm_clients::{LlmModel, grok::GrokClient};
use tokenmind_core::{task::Task, context::AppContext};
use crate::Agent;

pub struct GrokAgent;

#[async_trait]
impl Agent for GrokAgent {
    fn name(&self) -> &str { "GrokAgent (grok-3-mini)" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[GrokAgent] Executing: {}", task.description);

        let client = GrokClient::new(ctx.grok_key.clone());

        let file_context = build_file_context(task);

        let system = "You are an expert software developer. Write clean, correct code. \
            Return only the code or content changes, no lengthy explanations unless asked.";

        let user_msg = if file_context.is_empty() {
            format!("Task: {}", task.description)
        } else {
            format!("Task: {}\n\nFile context:\n{}\n\nProvide the complete updated code.",
                task.description, file_context)
        };

        let (response, usage) = client.complete(system, &user_msg, 4096).await?;

        ctx.record_usage(&LlmModel::Grok, &usage);

        if ctx.verbose {
            info!("[GrokAgent][verbose] Response:\n{}", &response[..response.len().min(2000)]);
        }
        match crate::builder_agent::write_and_track(&response, &ctx.project_path, task) {
            Ok(w) if !w.is_empty() => info!("[GrokAgent] Wrote {} file(s): {}", w.len(), w.join(", ")),
            Err(e) => info!("[GrokAgent] File write error: {}", e),
            _ => {}
        }

        let tokens = usage.input_tokens + usage.output_tokens;
        task.mark_done(response.clone(), tokens);

        info!("[GrokAgent] Done. Tokens: {}", tokens);
        Ok(response)
    }
}

fn build_file_context(task: &Task) -> String {
    const MAX_CHARS: usize = 20_000;
    let mut ctx = String::new();
    for path in &task.file_targets {
        if let Ok(content) = std::fs::read_to_string(path) {
            ctx.push_str(&format!("--- {}\n{}\n", path.display(), content));
            if ctx.len() >= MAX_CHARS { break; }
        }
    }
    ctx
}
