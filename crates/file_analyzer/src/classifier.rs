use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskCategory {
    /// Rust/Tauri backend .rs files — High priority
    TauriRust,
    /// Plain Rust .rs files — High priority
    RustCode,
    /// Vue / Tauri UI / JS / TS / HTML — Medium priority
    FrontendCode,
    /// Python / Go / other backend — Medium priority
    BackendLight,
    /// Markdown documentation — Low priority
    Documentation,
    /// JSON / TOML / YAML / sh / .env / .gitignore — Low priority
    Config,
    Mixed,
    Trivial,
    Unknown,
}

pub struct TaskClassifier;

// Files that indicate a Tauri project context
const TAURI_MARKERS: &[&str] = &["tauri.conf.json", "src-tauri", "tauri.conf", "Cargo.toml"];

impl TaskClassifier {
    pub fn classify(prompt: &str, file_paths: &[&Path]) -> TaskCategory {
        let prompt_lower = prompt.to_lowercase();

        let trivial_patterns = [
            "add comment", "rename variable", "fix typo", "format code",
            "add docstring", "rename", "typo", "comment", "whitespace",
        ];
        if trivial_patterns.iter().any(|p| prompt_lower.contains(p)) {
            return TaskCategory::Trivial;
        }

        if file_paths.is_empty() {
            return Self::classify_by_prompt(&prompt_lower);
        }

        let categories: Vec<TaskCategory> = file_paths.iter()
            .map(|p| Self::classify_by_extension(p))
            .collect();

        if categories.is_empty() {
            return TaskCategory::Unknown;
        }

        // All same category → return it
        if categories.iter().all(|c| c == &categories[0]) {
            return categories[0].clone();
        }

        // Mixed Rust + Frontend → treat as TauriRust (backend takes precedence)
        let has_rust     = categories.iter().any(|c| matches!(c, TaskCategory::RustCode | TaskCategory::TauriRust));
        let has_frontend = categories.iter().any(|c| c == &TaskCategory::FrontendCode);
        if has_rust && has_frontend {
            return TaskCategory::TauriRust;
        }

        TaskCategory::Mixed
    }

    pub fn classify_by_extension(path: &Path) -> TaskCategory {
        let ext      = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let path_str = path.to_str().unwrap_or("");

        // Config / low-priority files
        match ext.as_str() {
            "json" | "yaml" | "yml" | "toml" | "env" | "sh" | "bash" | "zsh" => {
                // tauri.conf.json is a Tauri config → treat as Rust tier
                if TAURI_MARKERS.iter().any(|m| filename.contains(m)) {
                    return TaskCategory::TauriRust;
                }
                // Cargo.toml → Rust
                if filename == "Cargo.toml" || filename == "Cargo.lock" {
                    return TaskCategory::RustCode;
                }
                return TaskCategory::Config;
            }
            "gitignore" | "dockerignore" | "editorconfig" | "nvmrc" => return TaskCategory::Config,
            _ => {}
        }

        // Documentation
        if matches!(ext.as_str(), "md" | "mdx" | "txt" | "rst") {
            return TaskCategory::Documentation;
        }

        // Rust — check for Tauri context
        if ext == "rs" {
            if path_str.contains("src-tauri") || path_str.contains("tauri") {
                return TaskCategory::TauriRust;
            }
            return TaskCategory::RustCode;
        }

        // Frontend / Vue / TS / JS
        if matches!(ext.as_str(), "vue" | "jsx" | "tsx" | "html" | "htm" | "css" | "scss" | "sass" | "js" | "ts") {
            return TaskCategory::FrontendCode;
        }

        // Other backend languages
        if matches!(ext.as_str(), "py" | "go" | "rb" | "php" | "java" | "kt") {
            return TaskCategory::BackendLight;
        }

        // Special filenames
        match filename {
            "package.json" | "vite.config.ts" | "vite.config.js" => return TaskCategory::FrontendCode,
            _ => {}
        }

        TaskCategory::Unknown
    }

    fn classify_by_prompt(prompt: &str) -> TaskCategory {
        let rust_kw     = ["rust", "cargo", "impl", "trait", "lifetime", "borrow", "async fn", "tokio", "actix", "tauri backend"];
        let tauri_kw    = ["tauri", "src-tauri"];
        let frontend_kw = ["vue", "react", "angular", "component", "html", "css", "tailwind", "typescript", "vite", "frontend"];
        let doc_kw      = ["readme", "documentation", "changelog", "docstring"];
        let cfg_kw      = ["config", ".env", ".json", ".toml", ".yaml", "setting"];

        if tauri_kw.iter().any(|k| prompt.contains(k)) {
            // "tauri" alone is ambiguous — backend context → TauriRust
            return TaskCategory::TauriRust;
        }
        if rust_kw.iter().any(|k| prompt.contains(k)) {
            return TaskCategory::RustCode;
        }
        if frontend_kw.iter().any(|k| prompt.contains(k)) {
            return TaskCategory::FrontendCode;
        }
        if doc_kw.iter().any(|k| prompt.contains(k)) {
            return TaskCategory::Documentation;
        }
        if cfg_kw.iter().any(|k| prompt.contains(k)) {
            return TaskCategory::Config;
        }

        TaskCategory::Unknown
    }

    pub fn is_trivial_prompt(prompt: &str) -> bool {
        let trivial_patterns = [
            "add comment", "fix typo", "typo", "format code", "reformat",
            "add docstring", "rename variable", "rename function",
        ];
        let lower = prompt.to_lowercase();
        trivial_patterns.iter().any(|p| lower.contains(p))
    }
}
