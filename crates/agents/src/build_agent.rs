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
    fn default() -> Self { Self::new() }
}

// ── Test runner ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct TestResult {
    pub success: bool,
    pub passed: u32,
    pub failed: u32,
    pub output: String,
}

impl BuildValidator {
    /// Run the test suite after a successful build.
    pub fn run_tests(&self, project_root: &std::path::Path, language: &str) -> TestResult {
        info!("[BuildValidator] Running tests for {} at {}", language, project_root.display());
        match language {
            "rust" => self.cargo_test(project_root),
            "node" | "vue" | "typescript" => self.npm_test(project_root),
            "python" => self.pytest(project_root),
            "go" => self.go_test(project_root),
            _ => TestResult { success: true, passed: 0, failed: 0, output: "No test runner for this language.".to_string() },
        }
    }

    fn cargo_test(&self, root: &std::path::Path) -> TestResult {
        let out = Command::new("cargo")
            .args(["test", "--no-fail-fast", "2>&1"])
            .current_dir(root)
            .output();

        let combined = match out {
            Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
            Err(e) => return TestResult { success: false, passed: 0, failed: 1, output: e.to_string() },
        };

        let (passed, failed) = parse_cargo_test_counts(&combined);
        let success = failed == 0;
        info!("[BuildValidator] cargo test: {} passed, {} failed", passed, failed);
        TestResult { success, passed, failed, output: combined }
    }

    fn npm_test(&self, root: &std::path::Path) -> TestResult {
        let out = Command::new("npm")
            .args(["test", "--", "--passWithNoTests"])
            .current_dir(root)
            .output();

        match out {
            Ok(o) => {
                let combined = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let success = o.status.success();
                TestResult { success, passed: 0, failed: 0, output: combined }
            }
            Err(e) => TestResult { success: false, passed: 0, failed: 1, output: e.to_string() },
        }
    }

    fn pytest(&self, root: &std::path::Path) -> TestResult {
        let out = Command::new("python3")
            .args(["-m", "pytest", "--tb=short", "-q"])
            .current_dir(root)
            .output();

        match out {
            Ok(o) => {
                let combined = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let success = o.status.success();
                TestResult { success, passed: 0, failed: 0, output: combined }
            }
            Err(e) => TestResult { success: false, passed: 0, failed: 1, output: e.to_string() },
        }
    }

    fn go_test(&self, root: &std::path::Path) -> TestResult {
        let out = Command::new("go")
            .args(["test", "./...", "-v"])
            .current_dir(root)
            .output();

        match out {
            Ok(o) => {
                let combined = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let success = o.status.success();
                TestResult { success, passed: 0, failed: 0, output: combined }
            }
            Err(e) => TestResult { success: false, passed: 0, failed: 1, output: e.to_string() },
        }
    }
}

fn parse_cargo_test_counts(output: &str) -> (u32, u32) {
    // "test result: ok. 5 passed; 0 failed;"
    for line in output.lines() {
        if line.contains("test result") {
            let passed = extract_count(line, "passed");
            let failed = extract_count(line, "failed");
            return (passed, failed);
        }
    }
    (0, 0)
}

fn extract_count(line: &str, keyword: &str) -> u32 {
    line.split(';')
        .find(|seg| seg.contains(keyword))
        .and_then(|seg| seg.split_whitespace().find(|w| w.chars().all(|c| c.is_ascii_digit())))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}
