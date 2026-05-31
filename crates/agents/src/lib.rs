pub mod rust_agent;
pub mod frontend_agent;
pub mod doc_agent;
pub mod build_agent;
pub mod local_agent;
pub mod grok_agent;
pub mod gpt_agent;
pub mod builder_agent;
pub mod verify_agent;

use async_trait::async_trait;
use anyhow::Result;
use tokenmind_core::task::Task;
use tokenmind_core::context::AppContext;

#[async_trait]
pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, task: &mut Task, ctx: &AppContext) -> Result<String>;
}
