use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use llm_clients::{LlmModel, Message, anthropic::AnthropicClient, collect_stream};
use tokenmind_core::{task::Task, context::AppContext};
use crate::{Agent, context_builder::build_rich_context, builder_agent::PATCH_FORMAT_INSTRUCTIONS};

pub struct FrontendAgent;

#[async_trait]
impl Agent for FrontendAgent {
    fn name(&self) -> &str { "FrontendAgent" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[FrontendAgent] Executing: {}", task.description);

        let client = AnthropicClient::new(ctx.anthropic_key.clone());

        let file_context = build_rich_context(task, ctx);

        let memory_prefix = if ctx.project_memory.is_empty() { String::new() }
            else { format!("## Project instructions\n{}\n\n", ctx.project_memory) };

        let system = format!(
            "{}You are an expert frontend developer specializing in Vue 3 (Composition API), \
            TypeScript, and Tailwind CSS. Write clean, typed, maintainable code. \
            For Python/Go tasks, write idiomatic code following best practices. \
            {}",
            memory_prefix, PATCH_FORMAT_INSTRUCTIONS
        );

        let user_msg = format!(
            "Task: {}\n\nContext:\n{}\n\nProvide the complete updated code.",
            task.description, file_context
        );

        let (response, usage) = if ctx.stream_sink.is_some() {
            let rx = client.complete_stream(&LlmModel::ClaudeHaiku, &system, vec![Message::user(user_msg)], 4096).await?;
            collect_stream(rx, ctx.stream_sink.as_ref(), task.id).await?
        } else {
            client.complete(&LlmModel::ClaudeHaiku, &system, vec![Message::user(user_msg)], 4096).await?
        };

        ctx.record_usage(&LlmModel::ClaudeHaiku, &usage);

        if ctx.verbose {
            info!("[FrontendAgent][verbose] Response:\n{}", &response[..response.len().min(2000)]);
        }
        match crate::builder_agent::write_and_track(&response, &ctx.project_path, task) {
            Ok(w) if !w.is_empty() => info!("[FrontendAgent] Wrote {} file(s): {}", w.len(), w.join(", ")),
            Err(e) => info!("[FrontendAgent] File write error: {}", e),
            _ => {}
        }

        let tokens = usage.input_tokens + usage.output_tokens;
        task.mark_done(response.clone(), tokens);

        info!("[FrontendAgent] Done. Tokens used: {}", tokens);
        Ok(response)
    }
}

