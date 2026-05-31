use std::path::Path;
use llm_clients::LlmModel;
use file_analyzer::classifier::{TaskClassifier, TaskCategory};
use crate::task::{Task, TaskType, Priority};
use crate::llm_arch_config::LlmArchConfig;

pub struct Dispatcher {
    pub local_confidence_threshold: f32,
    pub has_anthropic: bool,
    pub has_gemini: bool,
    pub has_grok: bool,
    pub has_gpt: bool,
    pub arch: LlmArchConfig,
}

impl Dispatcher {
    pub fn new(
        local_confidence_threshold: f32,
        has_anthropic: bool,
        has_gemini: bool,
        has_grok: bool,
        has_gpt: bool,
        arch: LlmArchConfig,
    ) -> Self {
        Self { local_confidence_threshold, has_anthropic, has_gemini, has_grok, has_gpt, arch }
    }

    fn has_key(&self, model: &LlmModel) -> bool {
        match model {
            LlmModel::ClaudeSonnet | LlmModel::ClaudeHaiku => self.has_anthropic,
            LlmModel::Gemini => self.has_gemini,
            LlmModel::Grok   => self.has_grok,
            LlmModel::Gpt    => self.has_gpt,
            LlmModel::Local  => true,
        }
    }

    /// Pick the first available model from a preferred + fallback list.
    fn resolve_tier(&self, preferred: &LlmModel, fallbacks: &[LlmModel]) -> LlmModel {
        if self.has_key(preferred) {
            return preferred.clone();
        }
        for fb in fallbacks {
            if self.has_key(fb) {
                return fb.clone();
            }
        }
        LlmModel::Local
    }

    /// Route a task to a model using the three-tier arch config.
    fn route_by_priority(&self, priority: &Priority) -> LlmModel {
        match priority {
            Priority::High   => self.resolve_tier(&self.arch.high.preferred,   &self.arch.high.fallbacks),
            Priority::Medium => self.resolve_tier(&self.arch.medium.preferred, &self.arch.medium.fallbacks),
            Priority::Low    => self.resolve_tier(&self.arch.low.preferred,    &self.arch.low.fallbacks),
        }
    }

    /// Classify files + prompt → TaskType + Priority → pick LLM.
    pub fn route(&self, description: &str, file_paths: &[&Path]) -> (TaskType, LlmModel) {
        if TaskClassifier::is_trivial_prompt(description) {
            return (TaskType::Trivial, LlmModel::Local);
        }

        let category = TaskClassifier::classify(description, file_paths);

        let task_type = match category {
            TaskCategory::RustCode     => TaskType::RustCode,
            TaskCategory::TauriRust    => TaskType::RustCode,
            TaskCategory::FrontendCode => TaskType::FrontendCode,
            TaskCategory::BackendLight => TaskType::BackendLight,
            TaskCategory::Documentation => TaskType::DocUpdate,
            TaskCategory::Config       => TaskType::ConfigEdit,
            TaskCategory::Trivial      => return (TaskType::Trivial, LlmModel::Local),
            TaskCategory::Mixed        => TaskType::RustCode,
            TaskCategory::Unknown      => TaskType::BackendLight,
        };

        let priority = task_type.default_priority();
        let model = self.route_by_priority(&priority);
        (task_type, model)
    }

    /// Build a Task from a description and optional file targets.
    pub fn build_task(&self, description: &str, file_paths: &[&Path]) -> Task {
        let (task_type, model) = self.route(description, file_paths);
        Task::new(description.to_string(), task_type, model)
            .with_files(file_paths.iter().map(|p| p.to_path_buf()).collect())
    }

    /// Split a mixed task into per-file sub-tasks.
    pub fn decompose_mixed(&self, description: &str, file_paths: &[&Path]) -> Vec<Task> {
        if file_paths.is_empty() {
            return vec![self.build_task(description, &[])];
        }

        file_paths.iter()
            .map(|path| {
                let (task_type, model) = self.route(description, &[path]);
                Task::new(
                    format!("{} ({})", description, path.display()),
                    task_type,
                    model,
                ).with_files(vec![path.to_path_buf()])
            })
            .collect()
    }
}
