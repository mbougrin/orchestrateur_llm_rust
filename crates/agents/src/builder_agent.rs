/// Creates files on disk from code blocks embedded in an LLM response.
/// The agent is invoked after a coder agent (RustAgent, FrontendAgent…) completes
/// and its result contains fenced code blocks with path annotations.
///
/// Supported block headers:
///   ```rust:src/main.rs      → writes src/main.rs
///   ```vue:src/App.vue       → writes src/App.vue
///   ```typescript:src/api.ts
///   ```json:package.json
///   (any language tag with a colon-separated path)
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use std::path::{Path, PathBuf};
use tracing::info;
use tokenmind_core::{task::Task, context::AppContext};
use crate::Agent;

pub struct BuilderAgent {
    /// Root directory where files are written (usually the project path).
    pub project_root: PathBuf,
}

impl BuilderAgent {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

#[async_trait]
impl Agent for BuilderAgent {
    fn name(&self) -> &str { "BuilderAgent (file writer)" }

    async fn execute(&self, task: &mut Task, _ctx: &AppContext) -> Result<String> {
        info!("[BuilderAgent] Creating files for: {}", task.description);

        // The task result field holds the coder's output (set by the upstream agent).
        let llm_output = task.result.clone().unwrap_or_default();
        if llm_output.is_empty() {
            anyhow::bail!("BuilderAgent: no LLM output to parse (task.result is empty)");
        }

        let files = extract_files(&llm_output);
        if files.is_empty() {
            anyhow::bail!("BuilderAgent: no fenced code blocks with path annotations found");
        }

        let mut written: Vec<String> = Vec::new();
        for (rel_path, content) in &files {
            let full_path = self.project_root.join(rel_path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full_path, content)?;
            info!("[BuilderAgent] Wrote: {}", full_path.display());
            written.push(rel_path.clone());
        }

        let summary = format!("Created {} file(s): {}", written.len(), written.join(", "));
        task.mark_done(summary.clone(), 0);
        Ok(summary)
    }
}

/// Extract `(path, content)` pairs from fenced code blocks annotated with a path.
/// Handles both `\`\`\`lang:path/to/file.ext` and `\`\`\`path/to/file.ext`.
pub fn extract_files(text: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Match ```lang:path or ```path/to/file (with extension in the "lang" slot)
    let re = Regex::new(r"```(?:[a-zA-Z0-9_+\-]*):([^\s`]+)\n([\s\S]*?)```").unwrap();
    for cap in re.captures_iter(text) {
        let path    = cap[1].trim().to_string();
        let content = cap[2].trim_end().to_string();
        if !path.is_empty() && seen.insert(path.clone()) {
            files.push((path, content));
        }
    }

    // If nothing annotated, fall back to plain blocks for single-file responses
    if files.is_empty() {
        let re_plain = Regex::new(r"```(?:rust|vue|typescript|ts|javascript|js|json|toml|sh|bash)\n([\s\S]*?)```").unwrap();
        for cap in re_plain.captures_iter(text) {
            let content = cap[1].trim_end().to_string();
            let path    = infer_path_from_content(&content);
            if seen.insert(path.clone()) {
                files.push((path, content));
            }
        }
    }

    files
}

fn infer_path_from_content(content: &str) -> String {
    if content.contains("fn main()") || content.contains("#[tauri::command]") {
        return "src/main.rs".to_string();
    }
    if content.contains("<template>") || content.contains("defineComponent") {
        return "src/App.vue".to_string();
    }
    if content.contains("[package]") {
        return "Cargo.toml".to_string();
    }
    if content.contains("\"scripts\"") || content.contains("\"dependencies\"") {
        return "package.json".to_string();
    }
    "src/main.rs".to_string()
}

/// Convenience: write all files extracted from `llm_output` under `root`.
/// Returns list of relative paths written.
pub fn write_files(llm_output: &str, root: &Path) -> Result<Vec<String>> {
    let files = extract_files(llm_output);
    let mut written = Vec::new();
    for (rel, content) in &files {
        let full = root.join(rel);
        if let Some(p) = full.parent() { std::fs::create_dir_all(p)?; }
        std::fs::write(&full, content)?;
        written.push(rel.clone());
    }
    Ok(written)
}
