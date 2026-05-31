/// Executes shell commands on behalf of agents.
/// Agents include TOOL: directives in their LLM output; `run_tool_loop` handles them.
use anyhow::Result;
use std::path::Path;
use tokio::process::Command;

pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

// Whitelist: only these command prefixes are allowed.
const ALLOWED: &[&str] = &["grep", "find", "git", "cargo", "npm", "ls", "cat", "wc", "rg", "fd"];
const BLOCKED: &[&str] = &["rm", "dd", "sudo", "curl", "wget", "mkfs", "shutdown", "reboot"];

pub struct ShellTool;

impl ShellTool {
    /// Run an arbitrary shell command under `cwd`.
    /// Returns an error if the command prefix is blocked.
    pub async fn run(cmd: &str, cwd: &Path) -> Result<ShellOutput> {
        let first = cmd.split_whitespace().next().unwrap_or("");
        if BLOCKED.iter().any(|b| first.starts_with(b)) {
            anyhow::bail!("ShellTool: command '{}' is blocked", first);
        }
        if !ALLOWED.iter().any(|a| first.starts_with(a)) {
            anyhow::bail!("ShellTool: command '{}' not in whitelist", first);
        }

        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .output()
            .await?;

        Ok(ShellOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            success: output.status.success(),
        })
    }

    pub async fn grep(pattern: &str, path: &Path, cwd: &Path) -> Result<String> {
        let cmd = format!("grep -rn --include='*' '{}' '{}'", pattern.replace('\'', "\\'"), path.display());
        let out = Self::run(&cmd, cwd).await?;
        Ok(out.stdout)
    }

    pub async fn find(name_pattern: &str, root: &Path, cwd: &Path) -> Result<String> {
        let cmd = format!("find '{}' -name '{}' -not -path '*/target/*' -not -path '*/node_modules/*'",
            root.display(), name_pattern);
        let out = Self::run(&cmd, cwd).await?;
        Ok(out.stdout)
    }

    pub async fn git_diff(cwd: &Path) -> Result<String> {
        let out = Self::run("git diff HEAD", cwd).await?;
        // Truncate to avoid overwhelming the LLM
        let diff = out.stdout;
        Ok(if diff.len() > 6000 { format!("{}\n[truncated…]", &diff[..6000]) } else { diff })
    }

    pub async fn git_status(cwd: &Path) -> Result<String> {
        let out = Self::run("git status --short", cwd).await?;
        Ok(out.stdout)
    }

    pub async fn git_log(cwd: &Path, n: usize) -> Result<String> {
        let cmd = format!("git log --oneline -{}", n);
        let out = Self::run(&cmd, cwd).await?;
        Ok(out.stdout)
    }
}

// ── TOOL: directive parser ────────────────────────────────────────────────────

/// Check if `text` contains any `TOOL:` directive.
pub fn has_tool_call(text: &str) -> bool {
    text.contains("TOOL:")
}

/// Execute all `TOOL:` directives found in `text` under `cwd`.
/// Returns a formatted result block to feed back to the LLM.
pub async fn execute_tool_calls(text: &str, cwd: &Path) -> String {
    let mut results = String::new();

    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with("TOOL:") { continue; }
        let rest = &t[5..]; // strip "TOOL:"
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        let tool_name = parts.first().copied().unwrap_or("");

        let output = match tool_name {
            "grep" => {
                let pattern = parts.get(1).copied().unwrap_or("");
                let path    = parts.get(2).map(Path::new).unwrap_or(cwd);
                ShellTool::grep(pattern, path, cwd).await
                    .unwrap_or_else(|e| e.to_string())
            }
            "find" => {
                let pattern = parts.get(1).copied().unwrap_or("*");
                ShellTool::find(pattern, cwd, cwd).await
                    .unwrap_or_else(|e| e.to_string())
            }
            "git_diff" => ShellTool::git_diff(cwd).await.unwrap_or_else(|e| e.to_string()),
            "git_status" => ShellTool::git_status(cwd).await.unwrap_or_else(|e| e.to_string()),
            "git_log"    => ShellTool::git_log(cwd, 5).await.unwrap_or_else(|e| e.to_string()),
            "shell" => {
                let cmd = parts.get(1).copied().unwrap_or("");
                ShellTool::run(cmd, cwd).await
                    .map(|o| format!("{}{}", o.stdout, o.stderr))
                    .unwrap_or_else(|e| e.to_string())
            }
            _ => format!("Unknown tool: {}", tool_name),
        };

        results.push_str(&format!("## Result of TOOL:{}\n```\n{}\n```\n\n", rest, output.trim()));
    }

    results
}

/// The tool instructions to include in agent system prompts.
pub const TOOL_INSTRUCTIONS: &str = r#"
## Available tools
Before writing code, you may call these tools by including the directive on its own line:

TOOL:grep:<pattern>:<path>      — search for pattern in path
TOOL:find:<name_pattern>        — find files by name
TOOL:git_diff                   — show uncommitted changes
TOOL:git_status                 — show working tree status
TOOL:git_log                    — show last 5 commits
TOOL:shell:<command>            — run a whitelisted shell command

The tool results will be provided in the next message. Only use tools when necessary.
"#;
