use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use llm_clients::gemini::GeminiClient;
use llm_clients::{collect_stream, LlmModel};
use tokenmind_core::{task::Task, context::AppContext};
use crate::{Agent, context_builder::build_rich_context};

pub struct DocAgent;

#[async_trait]
impl Agent for DocAgent {
    fn name(&self) -> &str { "DocAgent" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[DocAgent] Executing: {}", task.description);

        let client = GeminiClient::new(ctx.gemini_key.clone());

        let file_context = build_rich_context(task, ctx);

        let memory_prefix = if ctx.project_memory.is_empty() { String::new() }
            else { format!("## Project instructions\n{}\n\n", ctx.project_memory) };

        let system = format!(
            "{}You are a technical writer. Write clear, accurate documentation in Markdown. \
            Keep it concise but complete. For README updates, maintain the existing style.",
            memory_prefix
        );

        let user_prompt = format!(
            "Task: {}\n\nExisting content:\n{}\n\nProvide the updated content.",
            task.description, file_context
        );

        let (response, usage) = if ctx.stream_sink.is_some() {
            let rx = client.complete_stream(&system, &user_prompt, 8192).await?;
            collect_stream(rx, ctx.stream_sink.as_ref(), task.id).await?
        } else {
            client.complete(&system, &user_prompt, 8192).await?
        };

        ctx.record_usage(&LlmModel::Gemini, &usage);

        if ctx.verbose {
            info!("[DocAgent][verbose] Response:\n{}", &response[..response.len().min(2000)]);
        }
        match crate::builder_agent::write_and_track(&response, &ctx.project_path, task) {
            Ok(w) if !w.is_empty() => info!("[DocAgent] Wrote {} file(s): {}", w.len(), w.join(", ")),
            Err(e) => info!("[DocAgent] File write error: {}", e),
            _ => {}
        }

        let tokens = usage.input_tokens + usage.output_tokens;
        task.mark_done(response.clone(), tokens);

        info!("[DocAgent] Done. Tokens used: {}", tokens);
        Ok(response)
    }
}

