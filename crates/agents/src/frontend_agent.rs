use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use llm_clients::{LlmModel, Message, anthropic::AnthropicClient};
use tokenmind_core::{task::Task, context::AppContext};
use crate::Agent;

const MAX_CONTEXT_CHARS: usize = 24_000;

pub struct FrontendAgent;

#[async_trait]
impl Agent for FrontendAgent {
    fn name(&self) -> &str { "FrontendAgent" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[FrontendAgent] Executing: {}", task.description);

        let client = AnthropicClient::new(ctx.anthropic_key.clone());

        let file_context = build_file_context(task);

        let system = "You are an expert frontend developer specializing in Vue 3 (Composition API), \
            TypeScript, and Tailwind CSS. Write clean, typed, maintainable code. \
            For Python/Go tasks, write idiomatic code following best practices. \
            Return only the code, no lengthy explanations.";

        let user_msg = format!(
            "Task: {}\n\nContext:\n{}\n\nProvide the complete updated code.",
            task.description, file_context
        );

        let (response, usage) = client
            .complete(&LlmModel::ClaudeHaiku, system, vec![Message::user(user_msg)], 4096)
            .await?;

        ctx.record_usage(&LlmModel::ClaudeHaiku, &usage);
        let tokens = usage.input_tokens + usage.output_tokens;
        task.mark_done(response.clone(), tokens);

        info!("[FrontendAgent] Done. Tokens used: {}", tokens);
        Ok(response)
    }
}

fn build_file_context(task: &Task) -> String {
    let mut context = String::new();
    for path in &task.file_targets {
        if let Ok(content) = std::fs::read_to_string(path) {
            let header = format!("// File: {}\n", path.display());
            let snippet = if content.len() > MAX_CONTEXT_CHARS {
                &content[..MAX_CONTEXT_CHARS]
            } else {
                &content
            };
            context.push_str(&header);
            context.push_str(snippet);
            context.push_str("\n\n");
        }
    }
    context
}
