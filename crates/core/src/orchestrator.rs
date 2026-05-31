use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use tracing::{info, warn};
use crate::context::AppContext;
use crate::dispatcher::Dispatcher;
use crate::task::{Task, TaskStatus};

pub struct Orchestrator {
    pub ctx: AppContext,
    dispatcher: Dispatcher,
    task_queue: Arc<Mutex<VecDeque<Task>>>,
    completed_ids: Arc<Mutex<Vec<Uuid>>>,
}

impl Orchestrator {
    pub fn new(ctx: AppContext) -> Self {
        let has_anthropic = !ctx.anthropic_key.is_empty() && !ctx.anthropic_key.contains("REPLACE_ME");
        let has_gemini    = !ctx.gemini_key.is_empty()    && !ctx.gemini_key.contains("REPLACE_ME");
        let has_grok      = !ctx.grok_key.is_empty()      && !ctx.grok_key.contains("REPLACE_ME");
        let has_gpt       = !ctx.gpt_key.is_empty()       && !ctx.gpt_key.contains("REPLACE_ME");
        Self {
            dispatcher: Dispatcher::new(
                ctx.local_confidence_threshold,
                has_anthropic, has_gemini, has_grok, has_gpt,
                ctx.llm_arch.clone(),
            ),
            ctx,
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            completed_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Parse a user prompt into a list of tasks (planning step via Sonnet).
    pub fn plan_from_prompt(&self, prompt: &str) -> Vec<Task> {
        info!("[Orchestrator] Planning tasks for: {}", prompt);

        // Simple heuristic planning: detect multiple intents separated by "+" or newlines
        let sub_prompts: Vec<&str> = prompt
            .split(|c| c == '+' || c == '\n')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if sub_prompts.len() <= 1 {
            vec![self.dispatcher.build_task(prompt, &[])]
        } else {
            sub_prompts.iter()
                .map(|p| self.dispatcher.build_task(p, &[]))
                .collect()
        }
    }

    pub fn enqueue_tasks(&self, tasks: Vec<Task>) {
        let mut queue = self.task_queue.lock().unwrap();
        for task in tasks {
            info!("[Orchestrator] Queued task: {} ({:?})", task.description, task.assigned_model);
            queue.push_back(task);
        }
    }

    pub fn next_ready_task(&self) -> Option<Task> {
        let completed = self.completed_ids.lock().unwrap().clone();
        let mut queue = self.task_queue.lock().unwrap();

        let pos = queue.iter().position(|t| {
            t.status == TaskStatus::Pending && t.is_ready(&completed)
        })?;

        let mut task = queue.remove(pos).unwrap();
        task.mark_running();
        Some(task)
    }

    pub fn complete_task(&self, task: Task) {
        if task.status == TaskStatus::Done {
            self.completed_ids.lock().unwrap().push(task.id);
            info!("[Orchestrator] Task done: {} (tokens: {})", task.description, task.tokens_used);
        } else {
            warn!("[Orchestrator] Task failed: {} — {:?}", task.description, task.error);
        }
    }

    pub fn cancel_pending(&self) {
        let mut queue = self.task_queue.lock().unwrap();
        queue.retain(|t| t.status != TaskStatus::Pending);
    }

    pub fn all_tasks_snapshot(&self) -> Vec<Task> {
        self.task_queue.lock().unwrap().iter().cloned().collect()
    }

    pub fn queue_len(&self) -> usize {
        self.task_queue.lock().unwrap().len()
    }
}
