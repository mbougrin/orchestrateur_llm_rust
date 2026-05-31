/// Builds a rich file context for LLM prompts:
/// - project tree
/// - git context (branch, status, recent diff)
/// - manually added files (/add)
/// - target files + 1-level imports
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use file_analyzer::{build_project_tree, resolve_deps};
use tokenmind_core::{context::AppContext, task::Task};

const MAX_CONTEXT_CHARS: usize = 32_000;

pub fn build_rich_context(task: &Task, ctx: &AppContext) -> String {
    let project_root = &ctx.project_path;
    let mut out = String::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    // Project tree header (compact, max 3 levels)
    let tree = build_project_tree(project_root);
    out.push_str(&format!("## Project structure\n```\n{}\n```\n\n", tree));

    // Git context
    let git_section = ctx.git.to_prompt_section();
    if !git_section.is_empty() {
        out.push_str(&git_section);
        out.push('\n');
    }

    // Manual context (/add files)
    if let Ok(manual) = ctx.manual_context.lock() {
        for (path, content) in manual.iter() {
            let remaining = MAX_CONTEXT_CHARS.saturating_sub(out.len());
            if remaining < 100 { break; }
            let header = format!("## Manually added: {}\n```\n", path);
            let budget = remaining.saturating_sub(header.len() + 6);
            out.push_str(&header);
            out.push_str(&content[..budget.min(content.len())]);
            out.push_str("\n```\n\n");
            seen.insert(PathBuf::from(path));
        }
    }

    // Level-1: target files + their direct imports (full content)
    let mut l1: Vec<(PathBuf, String)> = Vec::new();
    for target in &task.file_targets {
        if !target.exists() { continue; }
        for pair in resolve_deps(target, project_root) {
            if !l1.iter().any(|(p, _)| p == &pair.0) { l1.push(pair); }
        }
    }

    for (path, content) in &l1 {
        if seen.contains(path) { continue; }
        seen.insert(path.clone());

        let lang = lang_from_path(path);
        let header = format!("## File: {}\n```{}\n", path.display(), lang);
        let remaining = MAX_CONTEXT_CHARS.saturating_sub(out.len());
        if remaining < 100 { break; }

        if content.len() + header.len() + 4 <= remaining {
            out.push_str(&header);
            out.push_str(content);
            out.push_str("\n```\n\n");
        } else {
            let budget = remaining.saturating_sub(header.len() + 30);
            out.push_str(&header);
            out.push_str(&content[..budget.min(content.len())]);
            out.push_str("\n// [truncated]\n```\n\n");
            break;
        }
    }

    // Level-2: imports of level-1 files, public signatures only (saves budget)
    'l2: for (l1_path, l1_content) in &l1 {
        for l2_path in file_analyzer::parse_imports(l1_path, l1_content) {
            if !l2_path.starts_with(project_root) { continue; }
            if seen.contains(&l2_path) { continue; }
            seen.insert(l2_path.clone());
            let Ok(content) = std::fs::read_to_string(&l2_path) else { continue };
            let sigs = extract_signatures(&content);
            if sigs.is_empty() { continue; }
            let lang = lang_from_path(&l2_path);
            let header = format!("## Deps (sigs): {}\n```{}\n", l2_path.display(), lang);
            let remaining = MAX_CONTEXT_CHARS.saturating_sub(out.len());
            if remaining < 100 { break 'l2; }
            out.push_str(&header);
            out.push_str(&sigs);
            out.push_str("\n```\n\n");
        }
    }

    out
}

fn extract_signatures(content: &str) -> String {
    content.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("pub fn ")
                || t.starts_with("pub async fn ")
                || t.starts_with("pub struct ")
                || t.starts_with("pub enum ")
                || t.starts_with("pub trait ")
                || t.starts_with("pub type ")
                || t.starts_with("pub const ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn lang_from_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs"               => "rust",
        "ts" | "tsx"       => "typescript",
        "js" | "jsx"       => "javascript",
        "vue"              => "vue",
        "py"               => "python",
        "go"               => "go",
        "json"             => "json",
        "toml"             => "toml",
        "md"               => "markdown",
        "sh" | "bash"      => "bash",
        _                  => "",
    }
}
