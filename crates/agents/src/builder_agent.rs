/// Creates / patches files on disk from LLM output.
///
/// Supported formats (in priority order):
///
/// 1. New patch format (preferred — sent in system prompt):
///    <<<FILE:path/to/file.rs:CREATE>>>
///    <full file content>
///    <<<END>>>
///
///    <<<FILE:src/main.rs:REPLACE:42:67>>>
///    // replacement code for lines 42–67
///    <<<END>>>
///
/// 2. Legacy annotated fences (kept for backwards compat):
///    ```rust:src/main.rs
///    <content>
///    ```
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use std::path::{Path, PathBuf};
use tracing::info;
use tokenmind_core::{task::Task, context::AppContext};
use crate::Agent;

pub struct BuilderAgent {
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
        info!("[BuilderAgent] Creating/patching files for: {}", task.description);

        let llm_output = task.result.clone().unwrap_or_default();
        if llm_output.is_empty() {
            anyhow::bail!("BuilderAgent: no LLM output (task.result empty)");
        }

        let ops = extract_file_ops(&llm_output);
        if ops.is_empty() {
            anyhow::bail!("BuilderAgent: no file operations found in LLM output");
        }

        let auto_write = _ctx.auto_write;
        let mut written: Vec<String> = Vec::new();
        let mut pending_confirm: Vec<(String, String, String)> = Vec::new(); // (path, old, new)

        for op in &ops {
            let full_path = self.project_root.join(&op.path);
            let (old, new) = apply_op(&full_path, op)?;

            // Flag large changes if auto_write is off
            if !auto_write && !old.is_empty() {
                let ratio = change_ratio(&old, &new);
                if ratio > 0.30 {
                    info!("[BuilderAgent] Large change ({:.0}%) in {} — flagged for review", ratio * 100.0, op.path);
                    pending_confirm.push((op.path.clone(), old.clone(), new.clone()));
                }
            }

            task.file_diffs.push((op.path.clone(), old, new));
            info!("[BuilderAgent] {:?}: {}", op.kind, full_path.display());
            written.push(op.path.clone());
        }

        let summary = if pending_confirm.is_empty() {
            format!("{} file(s) written: {}", written.len(), written.join(", "))
        } else {
            format!("{} file(s) written ({} large change(s) — /diff to review): {}",
                written.len(), pending_confirm.len(), written.join(", "))
        };

        task.mark_done(summary.clone(), 0);
        Ok(summary)
    }
}

// ── File operation types ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum OpKind {
    Create,
    Replace { start: usize, end: usize },
}

pub struct FileOp {
    pub path: String,
    pub kind: OpKind,
    pub content: String,
}

// ── Main parser ───────────────────────────────────────────────────────────────

pub fn extract_file_ops(text: &str) -> Vec<FileOp> {
    // Try new patch format first
    let ops = parse_patch_format(text);
    if !ops.is_empty() { return ops; }

    // Fall back to legacy annotated fences
    parse_legacy_fences(text)
}

/// Parse `<<<FILE:path:CREATE>>>...<<<END>>>` and `<<<FILE:path:REPLACE:s:e>>>...<<<END>>>`.
fn parse_patch_format(text: &str) -> Vec<FileOp> {
    let re = Regex::new(r"<<<FILE:([^:>]+):([^>]+)>>>\n([\s\S]*?)<<<END>>>").unwrap();
    let mut ops = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for cap in re.captures_iter(text) {
        let path    = cap[1].trim().to_string();
        let spec    = cap[2].trim();
        let content = cap[3].trim_end_matches('\n').to_string();

        if seen.contains(&path) { continue; }

        let kind = if spec.eq_ignore_ascii_case("create") {
            OpKind::Create
        } else if spec.to_ascii_uppercase().starts_with("REPLACE:") {
            let parts: Vec<&str> = spec[8..].splitn(2, ':').collect();
            let start = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1);
            let end   = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(start);
            OpKind::Replace { start, end }
        } else {
            // Unknown spec → treat as create
            OpKind::Create
        };

        seen.insert(path.clone());
        ops.push(FileOp { path, kind, content });
    }

    ops
}

/// Fallback: parse legacy ` ```lang:path/to/file` blocks.
fn parse_legacy_fences(text: &str) -> Vec<FileOp> {
    let mut ops = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let re = Regex::new(r"```(?:[a-zA-Z0-9_+\-]*):([^\s`]+)\n([\s\S]*?)```").unwrap();
    for cap in re.captures_iter(text) {
        let path    = cap[1].trim().to_string();
        let content = cap[2].trim_end().to_string();
        if !path.is_empty() && seen.insert(path.clone()) {
            ops.push(FileOp { path, kind: OpKind::Create, content });
        }
    }

    if ops.is_empty() {
        // Last resort: plain code fences — infer path from content
        let re_plain = Regex::new(r"```(?:rust|vue|typescript|ts|javascript|js|json|toml|sh|bash)\n([\s\S]*?)```").unwrap();
        for cap in re_plain.captures_iter(text) {
            let content = cap[1].trim_end().to_string();
            let path    = infer_path_from_content(&content);
            if seen.insert(path.clone()) {
                ops.push(FileOp { path, kind: OpKind::Create, content });
            }
        }
    }

    ops
}

// ── Apply a single file operation ─────────────────────────────────────────────

/// Apply an op and return (old_content, new_content) for the diff.
fn apply_op(full_path: &Path, op: &FileOp) -> Result<(String, String)> {
    let old = std::fs::read_to_string(full_path).unwrap_or_default();

    let new = match &op.kind {
        OpKind::Create => {
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            op.content.clone()
        }
        OpKind::Replace { start, end } => {
            apply_patch_lines(&old, *start, *end, &op.content)
        }
    };

    std::fs::write(full_path, &new)?;
    Ok((old, new))
}

/// Returns true if the change ratio exceeds `threshold` (0.0..1.0).
pub fn change_ratio(old: &str, new: &str) -> f64 {
    let old_len = old.lines().count().max(1);
    let new_len = new.lines().count();
    let changed = old_len.abs_diff(new_len) + old.lines().zip(new.lines()).filter(|(a, b)| a != b).count();
    (changed as f64 / old_len as f64).min(1.0)
}

/// Replace lines [start..=end] (1-indexed) in `original` with `replacement`.
fn apply_patch_lines(original: &str, start: usize, end: usize, replacement: &str) -> String {
    let lines: Vec<&str> = original.lines().collect();
    let s = start.saturating_sub(1).min(lines.len());
    let e = end.min(lines.len());
    let mut result: Vec<&str> = lines[..s].to_vec();
    result.extend(replacement.lines());
    result.extend_from_slice(&lines[e..]);
    result.join("\n")
}

fn infer_path_from_content(content: &str) -> String {
    if content.contains("fn main()") || content.contains("#[tauri::command]") { return "src/main.rs".to_string(); }
    if content.contains("<template>") || content.contains("defineComponent") { return "src/App.vue".to_string(); }
    if content.contains("[package]") { return "Cargo.toml".to_string(); }
    if content.contains("\"scripts\"") || content.contains("\"dependencies\"") { return "package.json".to_string(); }
    "src/main.rs".to_string()
}

// ── Convenience API ───────────────────────────────────────────────────────────

/// Extract file ops from `llm_output`, write them under `root`, and record diffs in `task`.
/// Returns paths of files written. No-ops silently when there are no patch blocks.
pub fn write_and_track(llm_output: &str, root: &Path, task: &mut tokenmind_core::task::Task) -> Result<Vec<String>> {
    let ops = extract_file_ops(llm_output);
    let mut written = Vec::new();
    for op in &ops {
        let full = root.join(&op.path);
        let (old, new) = apply_op(&full, op)?;
        task.file_diffs.push((op.path.clone(), old, new));
        written.push(op.path.clone());
    }
    Ok(written)
}

/// Write all operations extracted from `llm_output` under `root`. Returns relative paths.
pub fn write_files(llm_output: &str, root: &Path) -> Result<Vec<String>> {
    let ops = extract_file_ops(llm_output);
    let mut written = Vec::new();
    for op in &ops {
        let full = root.join(&op.path);
        apply_op(&full, op)?;
        written.push(op.path.clone());
    }
    Ok(written)
}

/// The system prompt instruction to include in coder agents.
pub const PATCH_FORMAT_INSTRUCTIONS: &str = r#"
## Output format
For every file you create or modify, use this exact format:

Create a new file:
<<<FILE:path/to/file.rs:CREATE>>>
<full file content>
<<<END>>>

Replace lines 42–67 in an existing file:
<<<FILE:src/main.rs:REPLACE:42:67>>>
<replacement code>
<<<END>>>

Multiple files in one response are supported — emit one block per file.
"#;
