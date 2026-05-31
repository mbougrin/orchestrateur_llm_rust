use anyhow::Result;
use std::path::{Path, PathBuf};
use ignore::WalkBuilder;

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: PathBuf,
    pub language: String,
    pub size_bytes: u64,
    pub line_count: usize,
    pub complexity_score: u32,
    pub public_symbols: Vec<String>,
}

pub struct ProjectScanner {
    root: PathBuf,
}

impl ProjectScanner {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn scan(&self) -> Result<Vec<FileInfo>> {
        let mut files = Vec::new();

        for entry in WalkBuilder::new(&self.root)
            .hidden(false)
            .ignore(true)
            .git_ignore(true)
            .build()
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }

            let path = entry.path().to_path_buf();

            if self.should_skip(&path) {
                continue;
            }

            if let Ok(info) = self.analyze_file(&path) {
                files.push(info);
            }
        }

        Ok(files)
    }

    pub fn generate_project_summary(&self) -> Result<String> {
        let files = self.scan()?;
        let mut by_lang: std::collections::HashMap<String, Vec<&FileInfo>> = std::collections::HashMap::new();

        for file in &files {
            by_lang.entry(file.language.clone()).or_default().push(file);
        }

        let mut summary = format!("Project at: {}\n\n", self.root.display());
        summary.push_str(&format!("Files: {} total\n\n", files.len()));

        for (lang, lang_files) in &by_lang {
            summary.push_str(&format!(
                "{}: {} files ({} lines total)\n",
                lang,
                lang_files.len(),
                lang_files.iter().map(|f| f.line_count).sum::<usize>()
            ));
        }

        summary.push_str("\nKey Rust symbols:\n");
        for file in files.iter().filter(|f| f.language == "rust").take(10) {
            for sym in &file.public_symbols {
                summary.push_str(&format!("  - {} ({})\n", sym, file.path.display()));
            }
        }

        Ok(summary)
    }

    fn should_skip(&self, path: &Path) -> bool {
        let skip_dirs = [".git", "target", "node_modules", ".tokenMind", "dist", "__pycache__"];
        path.components().any(|c| {
            skip_dirs.iter().any(|s| c.as_os_str() == *s)
        })
    }

    fn analyze_file(&self, path: &Path) -> Result<FileInfo> {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let language = detect_language(path, &content);
        let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let line_count = content.lines().count();
        let complexity_score = compute_complexity(&content, &language);
        let public_symbols = if language == "rust" {
            extract_rust_symbols(&content)
        } else {
            vec![]
        };

        Ok(FileInfo { path: path.to_path_buf(), language, size_bytes, line_count, complexity_score, public_symbols })
    }
}

fn detect_language(path: &Path, content: &str) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    match ext {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" => "javascript",
        "ts" | "mts" => "typescript",
        "vue" => "vue",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" => "css",
        "go" => "go",
        "rb" => "ruby",
        "php" => "php",
        "md" | "mdx" => "markdown",
        "sh" | "bash" | "zsh" => "shell",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "txt" | "rst" => "text",
        _ => {
            if filename == "Cargo.toml" || filename == "Cargo.lock" { return "toml".to_string(); }
            if filename == "package.json" { return "json".to_string(); }
            if content.starts_with("#!/bin/bash") || content.starts_with("#!/usr/bin/env bash") {
                return "shell".to_string();
            }
            "unknown"
        }
    }.to_string()
}

fn compute_complexity(content: &str, language: &str) -> u32 {
    let mut score = 0u32;
    match language {
        "rust" => {
            for line in content.lines() {
                let t = line.trim();
                if t.starts_with("fn ") || t.starts_with("pub fn ") { score += 2; }
                if t.starts_with("if ") || t.starts_with("} else") { score += 1; }
                if t.starts_with("match ") { score += 2; }
                if t.starts_with("for ") || t.starts_with("while ") { score += 1; }
            }
        }
        "python" => {
            for line in content.lines() {
                let t = line.trim();
                if t.starts_with("def ") || t.starts_with("async def ") { score += 2; }
                if t.starts_with("if ") || t.starts_with("elif ") { score += 1; }
                if t.starts_with("for ") || t.starts_with("while ") { score += 1; }
            }
        }
        _ => {
            score = content.lines().count() as u32 / 20;
        }
    }
    score
}

fn extract_rust_symbols(content: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("pub fn ") || t.starts_with("pub async fn ") {
            if let Some(name) = t.split('(').next().and_then(|s| s.split_whitespace().last()) {
                symbols.push(format!("fn {}", name));
            }
        } else if t.starts_with("pub struct ") {
            if let Some(name) = t.split_whitespace().nth(2) {
                let name = name.trim_end_matches('{').trim_end_matches('<').trim();
                symbols.push(format!("struct {}", name));
            }
        } else if t.starts_with("pub trait ") {
            if let Some(name) = t.split_whitespace().nth(2) {
                let name = name.trim_end_matches('{').trim_end_matches('<').trim();
                symbols.push(format!("trait {}", name));
            }
        } else if t.starts_with("pub enum ") {
            if let Some(name) = t.split_whitespace().nth(2) {
                let name = name.trim_end_matches('{').trim_end_matches('<').trim();
                symbols.push(format!("enum {}", name));
            }
        }
    }
    symbols
}
