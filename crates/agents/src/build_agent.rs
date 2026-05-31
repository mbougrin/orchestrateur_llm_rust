use std::process::Command;
use std::path::Path;
use tracing::info;

pub struct BuildValidator;

#[derive(Debug)]
pub struct BuildResult {
    pub success: bool,
    pub output: String,
    pub language: String,
}

impl BuildValidator {
    pub fn new() -> Self {
        Self
    }

    pub fn validate(&self, project_root: &Path, language: &str) -> BuildResult {
        info!("[BuildValidator] Validating {} project at {}", language, project_root.display());

        match language {
            "rust" => self.run_cargo_check(project_root),
            "node" | "vue" | "typescript" => self.run_npm_check(project_root),
            "python" => self.run_python_check(project_root),
            "go" => self.run_go_check(project_root),
            _ => BuildResult { success: true, output: "No validator for this language.".to_string(), language: language.to_string() },
        }
    }

    pub fn validate_rust(&self, project_root: &Path) -> BuildResult {
        self.run_cargo_check(project_root)
    }

    fn run_cargo_check(&self, root: &Path) -> BuildResult {
        let output = Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(root)
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let success = out.status.success();
                let combined = format!("{}{}", stdout, stderr);
                info!("[BuildValidator] cargo check: {}", if success { "OK" } else { "FAILED" });
                BuildResult { success, output: combined, language: "rust".to_string() }
            }
            Err(e) => BuildResult {
                success: false,
                output: format!("Failed to run cargo check: {}", e),
                language: "rust".to_string(),
            },
        }
    }

    fn run_npm_check(&self, root: &Path) -> BuildResult {
        let output = Command::new("npm")
            .args(["run", "type-check"])
            .current_dir(root)
            .output();

        match output {
            Ok(out) => {
                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                let success = out.status.success();
                info!("[BuildValidator] npm type-check: {}", if success { "OK" } else { "FAILED" });
                BuildResult { success, output: combined, language: "node".to_string() }
            }
            Err(e) => BuildResult {
                success: false,
                output: format!("Failed to run npm: {}", e),
                language: "node".to_string(),
            },
        }
    }

    fn run_python_check(&self, root: &Path) -> BuildResult {
        let output = Command::new("python3")
            .args(["-m", "py_compile"])
            .current_dir(root)
            .output();

        match output {
            Ok(out) => {
                let success = out.status.success();
                let combined = String::from_utf8_lossy(&out.stderr).to_string();
                BuildResult { success, output: combined, language: "python".to_string() }
            }
            Err(e) => BuildResult {
                success: false,
                output: format!("Failed to run python: {}", e),
                language: "python".to_string(),
            },
        }
    }

    fn run_go_check(&self, root: &Path) -> BuildResult {
        let output = Command::new("go")
            .args(["build", "./..."])
            .current_dir(root)
            .output();

        match output {
            Ok(out) => {
                let success = out.status.success();
                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                BuildResult { success, output: combined, language: "go".to_string() }
            }
            Err(e) => BuildResult {
                success: false,
                output: format!("Failed to run go build: {}", e),
                language: "go".to_string(),
            },
        }
    }
}

impl Default for BuildValidator {
    fn default() -> Self {
        Self::new()
    }
}
