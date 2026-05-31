use std::path::{Path, PathBuf};

/// Parse direct local imports/deps from a source file (1 level deep).
/// Returns absolute paths of files imported by `path`.
pub fn parse_imports(path: &Path, content: &str) -> Vec<PathBuf> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "rs"              => parse_rust_imports(path, content),
        "ts" | "tsx"      => parse_ts_imports(path, content),
        "js" | "jsx"      => parse_ts_imports(path, content),
        "vue"             => parse_vue_imports(path, content),
        _                 => vec![],
    }
}

/// Resolve 1-level dep tree: return (path, content) pairs for the target file
/// plus all its direct imports, staying within `project_root`.
pub fn resolve_deps(path: &Path, project_root: &Path) -> Vec<(PathBuf, String)> {
    let mut result = Vec::new();

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return result,
    };

    result.push((path.to_path_buf(), content.clone()));

    for dep_path in parse_imports(path, &content) {
        // Only include files inside the project
        if !dep_path.starts_with(project_root) { continue; }
        if let Ok(dep_content) = std::fs::read_to_string(&dep_path) {
            result.push((dep_path, dep_content));
        }
    }

    result
}

/// Build a compact file tree string for a project (ignores target/, node_modules/).
pub fn build_project_tree(root: &Path) -> String {
    let skip = ["target", "node_modules", ".git", "dist", "__pycache__", ".tokenMind"];
    let mut lines = vec![format!("{}/", root.file_name().and_then(|n| n.to_str()).unwrap_or("."))];

    let Ok(walker) = std::fs::read_dir(root) else { return lines.join("\n") };
    let mut entries: Vec<_> = walker.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());

    walk_tree(root, root, &skip, &mut lines, 1, 3);
    lines.join("\n")
}

fn walk_tree(root: &Path, dir: &Path, skip: &[&str], out: &mut Vec<String>, depth: usize, max_depth: usize) {
    if depth > max_depth { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        if skip.iter().any(|s| *s == name) { continue; }
        let indent = "  ".repeat(depth);
        if path.is_dir() {
            out.push(format!("{}{}/", indent, name));
            walk_tree(root, &path, skip, out, depth + 1, max_depth);
        } else {
            out.push(format!("{}{}", indent, name));
        }
    }
}

// ── Rust import parser ────────────────────────────────────────────────────────

fn parse_rust_imports(path: &Path, content: &str) -> Vec<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut deps = Vec::new();

    for line in content.lines() {
        let t = line.trim();

        // `mod foo;` → look for foo.rs or foo/mod.rs next to the current file
        if let Some(rest) = t.strip_prefix("mod ").and_then(|r| r.strip_suffix(';')) {
            let mod_name = rest.trim();
            let as_file = dir.join(format!("{}.rs", mod_name));
            let as_dir  = dir.join(mod_name).join("mod.rs");
            if as_file.exists() { deps.push(as_file); }
            else if as_dir.exists() { deps.push(as_dir); }
        }

        // `use crate::foo::bar` — try to resolve to a file (best-effort, no full name resolution)
        if t.starts_with("use crate::") {
            // find project root by walking up to Cargo.toml
            if let Some(root) = find_cargo_root(path) {
                let segments: Vec<&str> = t["use crate::".len()..]
                    .split("::")
                    .map(|s| s.trim_matches(['{', '}', ';', ' ', '\t']))
                    .collect();
                let candidate = root.join("src").join(segments[0]).with_extension("rs");
                if candidate.exists() { deps.push(candidate); }
            }
        }
    }

    deps
}

fn find_cargo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.parent()?;
    loop {
        if cur.join("Cargo.toml").exists() { return Some(cur.to_path_buf()); }
        cur = cur.parent()?;
    }
}

// ── TypeScript / JS import parser ─────────────────────────────────────────────

fn parse_ts_imports(path: &Path, content: &str) -> Vec<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut deps = Vec::new();

    for line in content.lines() {
        let t = line.trim();
        // Match: import ... from './something' or import ... from '../something'
        if !t.starts_with("import ") { continue; }
        let Some(from_pos) = t.rfind(" from ") else { continue };
        let spec = t[from_pos + 6..].trim().trim_matches(['"', '\'', ';']);
        if !spec.starts_with('.') { continue; } // skip node_modules

        let candidate_base = dir.join(spec);
        for ext in &["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx", "/index.js"] {
            let candidate = if ext.is_empty() {
                candidate_base.clone()
            } else if ext.starts_with('/') {
                candidate_base.join(&ext[1..])
            } else {
                PathBuf::from(format!("{}{}", candidate_base.display(), ext))
            };
            if candidate.exists() {
                deps.push(candidate);
                break;
            }
        }
    }

    deps
}

// ── Vue single-file component ─────────────────────────────────────────────────

fn parse_vue_imports(path: &Path, content: &str) -> Vec<PathBuf> {
    // A .vue file's <script> section uses TS/JS imports
    parse_ts_imports(path, content)
}
