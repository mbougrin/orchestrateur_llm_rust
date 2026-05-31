/// Runs the appropriate build command for the project and returns errors
/// if the build fails. Used after BuilderAgent writes files to disk.
/// On failure, the error message is meant to be fed back to the coder agent.
use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use tokenmind_core::{task::Task, context::AppContext};
use crate::{Agent, build_agent::BuildValidator};

pub struct VerifyAgent {
    pub project_root: std::path::PathBuf,
}

impl VerifyAgent {
    pub fn new(project_root: std::path::PathBuf) -> Self {
        Self { project_root }
    }

    /// Detect which build system to use from the files in task.file_targets.
    fn detect_language(task: &Task) -> &'static str {
        for path in &task.file_targets {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            match ext {
                "rs" => return "rust",
                "vue" | "ts" | "js" | "html" => return "node",
                "py" => return "python",
                "go" => return "go",
                _ => {}
            }
        }
        // Fallback: check for Cargo.toml or package.json in project root
        if std::path::Path::new("Cargo.toml").exists() { return "rust"; }
        if std::path::Path::new("package.json").exists() { return "node"; }
        "rust"
    }
}

#[async_trait]
impl Agent for VerifyAgent {
    fn name(&self) -> &str { "VerifyAgent (build checker)" }

    async fn execute(&self, task: &mut Task, _ctx: &AppContext) -> Result<String> {
        info!("[VerifyAgent] Verifying build at: {}", self.project_root.display());

        let lang = Self::detect_language(task);
        info!("[VerifyAgent] Detected language: {}", lang);

        let validator = BuildValidator::new();
        let result = tokio::task::spawn_blocking({
            let root = self.project_root.clone();
            let lang = lang.to_string();
            move || validator.validate(&root, &lang)
        }).await?;

        if !result.success {
            let errors = result.output;
            info!("[VerifyAgent] Build failed:\n{}", errors);
            anyhow::bail!("build_error:\n{}", errors);
        }

        // Build OK — now run tests
        let test_result = tokio::task::spawn_blocking({
            let root = self.project_root.clone();
            let lang = lang.to_string();
            let v2 = BuildValidator::new();
            move || v2.run_tests(&root, &lang)
        }).await?;

        if test_result.success {
            let msg = if test_result.passed > 0 {
                format!("Build OK + {} tests passed ({})", test_result.passed, lang)
            } else {
                format!("Build OK — no tests ({})", lang)
            };
            info!("[VerifyAgent] {}", msg);
            task.mark_done(msg.clone(), 0);
            Ok(msg)
        } else {
            let test_out = if test_result.output.len() > 3000 {
                format!("{}\n[truncated…]", &test_result.output[..3000])
            } else {
                test_result.output.clone()
            };
            info!("[VerifyAgent] Tests failed ({} failed)", test_result.failed);
            anyhow::bail!("test_error:\n## Tests échoués ({} failed)\n{}", test_result.failed, test_out);
        }
    }
}
