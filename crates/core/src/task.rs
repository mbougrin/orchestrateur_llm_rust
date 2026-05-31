use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;
use llm_clients::LlmModel;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Medium,
    High,
}

impl Priority {
    pub fn label(&self) -> &str {
        match self {
            Priority::High   => "high",
            Priority::Medium => "medium",
            Priority::Low    => "low",
        }
    }
}

impl Default for Priority {
    fn default() -> Self { Priority::Medium }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub description: String,
    pub file_targets: Vec<PathBuf>,
    pub task_type: TaskType,
    pub assigned_model: LlmModel,
    pub dependencies: Vec<Uuid>,
    pub priority: Priority,
    pub status: TaskStatus,
    pub retry_count: u8,
    pub tokens_used: u32,
    pub result: Option<String>,
    pub error: Option<String>,
}

impl Task {
    pub fn new(description: String, task_type: TaskType, model: LlmModel) -> Self {
        let priority = task_type.default_priority();
        Self {
            id: Uuid::new_v4(),
            description,
            file_targets: vec![],
            task_type,
            assigned_model: model,
            dependencies: vec![],
            priority,
            status: TaskStatus::Pending,
            retry_count: 0,
            tokens_used: 0,
            result: None,
            error: None,
        }
    }

    pub fn with_files(mut self, files: Vec<PathBuf>) -> Self {
        self.file_targets = files;
        self
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_dependencies(mut self, deps: Vec<Uuid>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn is_ready(&self, completed: &[Uuid]) -> bool {
        self.dependencies.iter().all(|d| completed.contains(d))
    }

    pub fn mark_running(&mut self) {
        self.status = TaskStatus::Running;
    }

    pub fn mark_done(&mut self, result: String, tokens: u32) {
        self.status = TaskStatus::Done;
        self.result = Some(result);
        self.tokens_used = tokens;
    }

    pub fn mark_failed(&mut self, error: String) {
        self.status = TaskStatus::Failed;
        self.error = Some(error);
        self.retry_count += 1;
    }

    pub fn can_retry(&self, max_retries: u8) -> bool {
        self.retry_count < max_retries
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskType {
    /// Rust / Tauri backend (.rs) — High priority → Sonnet
    RustCode,
    /// Vue / Tauri UI / JS / TS / HTML — Medium priority → Haiku
    FrontendCode,
    /// Python / Go / other backend — Medium priority → Haiku
    BackendLight,
    /// Markdown documentation — Low priority → Local
    DocUpdate,
    /// JSON / TOML / YAML / sh / .env — Low priority → Local
    ConfigEdit,
    /// Re-send code + build errors to coder — inherits parent priority
    BuildFix,
    /// Simple one-liner tasks — Low priority → Local
    Trivial,
    /// Architecture planning — High priority → Sonnet
    Planning,
    /// File creation from LLM blueprint — inherits parent priority
    FileCreation,
    /// Build verification — runs after FileCreation/RustCode tasks
    Verify,
}

impl TaskType {
    pub fn default_model(&self) -> LlmModel {
        match self {
            TaskType::RustCode | TaskType::Planning => LlmModel::ClaudeSonnet,
            TaskType::FrontendCode | TaskType::BackendLight | TaskType::ConfigEdit | TaskType::BuildFix => LlmModel::ClaudeHaiku,
            TaskType::DocUpdate | TaskType::Trivial | TaskType::FileCreation | TaskType::Verify => LlmModel::Local,
        }
    }

    pub fn default_priority(&self) -> Priority {
        match self {
            TaskType::RustCode | TaskType::Planning | TaskType::BuildFix => Priority::High,
            TaskType::FrontendCode | TaskType::BackendLight | TaskType::FileCreation | TaskType::Verify => Priority::Medium,
            TaskType::DocUpdate | TaskType::ConfigEdit | TaskType::Trivial => Priority::Low,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending   => write!(f, "pending"),
            TaskStatus::Running   => write!(f, "running"),
            TaskStatus::Done      => write!(f, "done"),
            TaskStatus::Failed    => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}
