/// Splits a file's content into logical chunks based on language.
pub struct Chunker;

impl Chunker {
    /// Chunk a file by functions (Rust), components (Vue), or sections (Markdown).
    /// Returns chunks as (title, content) pairs.
    pub fn chunk(content: &str, language: &str) -> Vec<(String, String)> {
        match language {
            "rust" => Self::chunk_rust(content),
            "vue" => Self::chunk_vue(content),
            "markdown" => Self::chunk_markdown(content),
            "python" => Self::chunk_python(content),
            _ => Self::chunk_by_size(content, 150),
        }
    }

    /// Extract only the function/impl containing a specific line range.
    pub fn extract_context_around(content: &str, target_line: usize, context_lines: usize) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let start = target_line.saturating_sub(context_lines);
        let end = (target_line + context_lines).min(lines.len());
        lines[start..end].join("\n")
    }

    /// Get function signatures only (for context without body).
    pub fn extract_signatures(content: &str, language: &str) -> String {
        match language {
            "rust" => {
                content.lines()
                    .filter(|l| {
                        let t = l.trim();
                        t.starts_with("pub fn ")
                            || t.starts_with("pub async fn ")
                            || t.starts_with("pub struct ")
                            || t.starts_with("pub trait ")
                            || t.starts_with("pub enum ")
                            || t.starts_with("impl ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => String::new(),
        }
    }

    fn chunk_rust(content: &str) -> Vec<(String, String)> {
        let mut chunks = Vec::new();
        let mut current_title = "module header".to_string();
        let mut current_chunk = Vec::new();
        let mut brace_depth = 0i32;

        for line in content.lines() {
            let trimmed = line.trim();

            // Detect function/impl/struct starts
            if brace_depth == 0 && (
                trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("impl ")
                || trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub trait ")
            ) {
                if !current_chunk.is_empty() {
                    chunks.push((current_title.clone(), current_chunk.join("\n")));
                    current_chunk.clear();
                }
                current_title = trimmed.split('{').next().unwrap_or(trimmed).trim().to_string();
            }

            brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
            brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;
            current_chunk.push(line);
        }

        if !current_chunk.is_empty() {
            chunks.push((current_title, current_chunk.join("\n")));
        }

        chunks
    }

    fn chunk_vue(content: &str) -> Vec<(String, String)> {
        let mut chunks = Vec::new();
        let sections = ["<template", "<script", "<style"];

        for section in &sections {
            if let Some(start) = content.find(section) {
                let end_tag = &format!("</{}", &section[1..]);
                let end = content[start..].find(end_tag.as_str())
                    .map(|e| start + e + end_tag.len())
                    .unwrap_or(content.len());
                chunks.push((section.trim_start_matches('<').to_string(), content[start..end].to_string()));
            }
        }

        if chunks.is_empty() {
            chunks.push(("full component".to_string(), content.to_string()));
        }

        chunks
    }

    fn chunk_markdown(content: &str) -> Vec<(String, String)> {
        let mut chunks = Vec::new();
        let mut current_title = "intro".to_string();
        let mut current_lines = Vec::new();

        for line in content.lines() {
            if line.starts_with('#') {
                if !current_lines.is_empty() {
                    chunks.push((current_title.clone(), current_lines.join("\n")));
                    current_lines.clear();
                }
                current_title = line.trim_start_matches('#').trim().to_string();
            }
            current_lines.push(line);
        }

        if !current_lines.is_empty() {
            chunks.push((current_title, current_lines.join("\n")));
        }

        chunks
    }

    fn chunk_python(content: &str) -> Vec<(String, String)> {
        let mut chunks = Vec::new();
        let mut current_title = "module".to_string();
        let mut current_lines = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("def ") || trimmed.starts_with("async def ") || trimmed.starts_with("class ") {
                if !current_lines.is_empty() {
                    chunks.push((current_title.clone(), current_lines.join("\n")));
                    current_lines.clear();
                }
                current_title = trimmed.split('(').next().unwrap_or(trimmed).to_string();
            }
            current_lines.push(line);
        }

        if !current_lines.is_empty() {
            chunks.push((current_title, current_lines.join("\n")));
        }

        chunks
    }

    fn chunk_by_size(content: &str, lines_per_chunk: usize) -> Vec<(String, String)> {
        content.lines()
            .collect::<Vec<_>>()
            .chunks(lines_per_chunk)
            .enumerate()
            .map(|(i, chunk)| (format!("chunk {}", i + 1), chunk.join("\n")))
            .collect()
    }
}
