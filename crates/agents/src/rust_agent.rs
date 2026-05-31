use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use llm_clients::{LlmModel, Message, anthropic::AnthropicClient};
use file_analyzer::chunker::Chunker;
use tokenmind_core::{task::Task, context::AppContext};
use crate::Agent;

const MAX_CONTEXT_TOKENS: usize = 8000;
const CHARS_PER_TOKEN: usize = 4;

pub struct RustAgent;

#[async_trait]
impl Agent for RustAgent {
    fn name(&self) -> &str { "RustAgent" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[RustAgent] Executing: {}", task.description);

        let client = AnthropicClient::new(ctx.anthropic_key.clone());

        let file_context = build_file_context(&task);

        let system = "You are an expert Rust developer. Write safe, idiomatic Rust code. \
            Focus on ownership correctness, avoid unnecessary clones, prefer Result over panic. \
            Return only the code changes, no explanations unless asked.";

        let user_msg = format!(
            "Task: {}\n\nFile context:\n{}\n\nProvide the complete updated code.",
            task.description, file_context
        );

        let messages = vec![Message::user(user_msg)];

        let (response, usage) = client
            .complete(&LlmModel::ClaudeSonnet, system, messages, 4096)
            .await?;

        ctx.record_usage(&LlmModel::ClaudeSonnet, &usage);

        let tokens = usage.input_tokens + usage.output_tokens;
        task.mark_done(response.clone(), tokens);

        info!("[RustAgent] Done. Tokens used: {}", tokens);
        Ok(response)
    }
}

fn build_file_context(task: &Task) -> String {
    let max_chars = MAX_CONTEXT_TOKENS * CHARS_PER_TOKEN;
    let mut context = String::new();

    for path in &task.file_targets {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let signatures = Chunker::extract_signatures(&content, "rust");
        let file_header = format!("// File: {}\n// Signatures:\n{}\n\n", path.display(), signatures);

        if context.len() + content.len() + file_header.len() < max_chars {
            context.push_str(&file_header);
            context.push_str(&content);
        } else {
            // Only send signatures if full file would exceed limit
            context.push_str(&file_header);
            context.push_str("// [File too large — sending signatures only]\n");
        }
    }

    context
}
