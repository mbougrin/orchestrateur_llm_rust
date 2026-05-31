use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use llm_clients::{LlmModel, Message, anthropic::AnthropicClient, collect_stream, estimate_tokens, estimate_cost_usd};
use tokenmind_core::{task::Task, context::AppContext};
use crate::{Agent, context_builder::build_rich_context, builder_agent::PATCH_FORMAT_INSTRUCTIONS,
            shell_tool::{execute_tool_calls, has_tool_call, TOOL_INSTRUCTIONS}};

pub struct RustAgent;

#[async_trait]
impl Agent for RustAgent {
    fn name(&self) -> &str { "RustAgent" }

    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String> {
        info!("[RustAgent] Executing: {}", task.description);

        let client = AnthropicClient::new(ctx.anthropic_key.clone());

        let file_context = build_rich_context(task, ctx);

        let memory_prefix = if ctx.project_memory.is_empty() { String::new() }
            else { format!("## Project instructions\n{}\n\n", ctx.project_memory) };

        let system = format!(
            "{}You are an expert Rust developer. Write safe, idiomatic Rust code. \
            Focus on ownership correctness, avoid unnecessary clones, prefer Result over panic. \
            {}\n{}",
            memory_prefix, TOOL_INSTRUCTIONS, PATCH_FORMAT_INSTRUCTIONS
        );

        let user_msg = format!(
            "Task: {}\n\nFile context:\n{}\n\nProvide the complete updated code.",
            task.description, file_context
        );

        let est = estimate_tokens(&system) + estimate_tokens(&user_msg);
        task.estimated_tokens = est as u32;
        info!("[RustAgent] Estimated: ~{} tokens (~${:.4})", est, estimate_cost_usd(est, &LlmModel::ClaudeSonnet));

        let mut messages = vec![Message::user(user_msg)];
        let mut total_usage = llm_clients::TokenUsage::default();

        // Tool loop: re-call LLM if it issues TOOL: directives (max 3 iterations)
        let response = loop {
            let (reply, usage) = if ctx.stream_sink.is_some() {
                let rx = client.complete_stream(&LlmModel::ClaudeSonnet, &system, messages.clone(), 4096).await?;
                collect_stream(rx, ctx.stream_sink.as_ref(), task.id).await?
            } else {
                client.complete(&LlmModel::ClaudeSonnet, &system, messages.clone(), 4096).await?
            };
            total_usage.input_tokens  += usage.input_tokens;
            total_usage.output_tokens += usage.output_tokens;

            if has_tool_call(&reply) && messages.len() < 7 {
                let tool_results = execute_tool_calls(&reply, &ctx.project_path).await;
                messages.push(Message::assistant(reply));
                messages.push(Message::user(format!("Tool results:\n\n{}\n\nNow write the code.", tool_results)));
            } else {
                break reply;
            }
        };

        ctx.record_usage(&LlmModel::ClaudeSonnet, &total_usage);

        if ctx.verbose {
            info!("[RustAgent][verbose] Response:\n{}", &response[..response.len().min(2000)]);
        }

        // Write files in one pass (T19 — multi-file creation)
        match crate::builder_agent::write_and_track(&response, &ctx.project_path, task) {
            Ok(written) if !written.is_empty() =>
                info!("[RustAgent] Wrote {} file(s): {}", written.len(), written.join(", ")),
            Err(e) => info!("[RustAgent] File write error: {}", e),
            _ => {}
        }

        let tokens = total_usage.input_tokens + total_usage.output_tokens;
        task.mark_done(response.clone(), tokens);

        info!("[RustAgent] Done. Tokens used: {}", tokens);
        Ok(response)
    }
}

