//! Python code execution — spawn, execute, timeout, collect results.
//!
//! Runs Python code in a subprocess with timeout enforcement.
//! Output is captured from stdout/stderr.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::sandbox::SandboxConfig;

/// Result of executing Python code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    /// Standard output from the script.
    pub stdout: String,
    /// Standard error output.
    pub stderr: String,
    /// Process exit code (0 = success).
    pub exit_code: i32,
    /// Execution time in milliseconds.
    pub execution_time_ms: u64,
    /// Whether execution was terminated due to timeout.
    pub timed_out: bool,
}

/// Python code runner with sandbox enforcement.
pub struct PythonRunner {
    workspace_path: PathBuf,
    sandbox: SandboxConfig,
}

impl PythonRunner {
    /// Create a new runner for the given workspace.
    pub fn new(workspace_path: PathBuf) -> Self {
        let sandbox = SandboxConfig::for_workspace(&workspace_path);
        Self {
            workspace_path,
            sandbox,
        }
    }

    /// Create a runner with custom sandbox config.
    pub fn with_config(workspace_path: PathBuf, sandbox: SandboxConfig) -> Self {
        Self {
            workspace_path,
            sandbox,
        }
    }

    /// Execute Python code string.
    ///
    /// 1. Validates code against sandbox rules.
    /// 2. Writes to a temp file (workspace/temp/code_{uuid}.py).
    /// 3. Spawns `python3 -u temp_file.py`.
    /// 4. Enforces timeout.
    /// 5. Captures stdout/stderr.
    /// 6. Cleans up temp file.
    pub async fn execute(&self, code: &str) -> Result<ExecutionResult> {
        // 1. Validate code
        self.sandbox.validate_code(code).map_err(|e| anyhow!("Sandbox violation: {}", e))?;

        // 2. Prepare temp file
        let temp_dir = self.workspace_path.join("temp");
        std::fs::create_dir_all(&temp_dir).context("Failed to create temp directory")?;

        let file_id = uuid::Uuid::new_v4().to_string();
        let temp_file = temp_dir.join(format!("code_{}.py", file_id));

        // Prepend sandbox preamble to user code
        let full_code = format!("{}\n# --- User Code ---\n{}", self.sandbox.preamble(), code);
        std::fs::write(&temp_file, &full_code).context("Failed to write temp Python file")?;

        // 3. Execute
        let result = self.run_python_file(&temp_file).await;

        // 4. Cleanup temp file
        let _ = std::fs::remove_file(&temp_file);

        result
    }

    /// Execute a Python file directly (must already exist).
    pub async fn execute_file(&self, file_path: &Path) -> Result<ExecutionResult> {
        if !file_path.exists() {
            return Err(anyhow!("Python file not found: {}", file_path.display()));
        }
        self.run_python_file(file_path).await
    }

    /// Internal: spawn python3 and run a file with timeout.
    async fn run_python_file(&self, file_path: &Path) -> Result<ExecutionResult> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(self.sandbox.timeout_seconds as u64);

        let mut child = Command::new("python3")
            .arg("-u") // unbuffered output
            .arg(file_path)
            .current_dir(&self.workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONIOENCODING", "utf-8")    // Force UTF-8 output on all platforms
            .env("PYTHONLEGACYWINDOWSSTDIO", "0") // Disable legacy Windows stdio
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn python3 process")?;

        // Take stdout/stderr handles out of the child so they can be read concurrently.
        // This avoids pipe buffer deadlock: if stdout fills its OS buffer while we
        // haven't started reading stderr, the process blocks and we deadlock.
        let mut child_stdout = child.stdout.take();
        let mut child_stderr = child.stderr.take();

        // Wait with timeout
        let result = tokio::time::timeout(timeout, async {
            let stdout_handle = async {
                let mut buf = Vec::new();
                if let Some(ref mut stdout) = child_stdout {
                    let _ = stdout.read_to_end(&mut buf).await;
                }
                buf
            };
            let stderr_handle = async {
                let mut buf = Vec::new();
                if let Some(ref mut stderr) = child_stderr {
                    let _ = stderr.read_to_end(&mut buf).await;
                }
                buf
            };

            let (stdout_buf, stderr_buf) = tokio::join!(stdout_handle, stderr_handle);
            let status = child.wait().await?;
            Ok::<_, anyhow::Error>((stdout_buf, stderr_buf, status))
        })
        .await;

        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok((stdout_buf, stderr_buf, status))) => {
                let mut stdout = String::from_utf8_lossy(&stdout_buf).to_string();
                let mut stderr = String::from_utf8_lossy(&stderr_buf).to_string();

                // Truncate if too large (char-boundary safe)
                if stdout.len() > self.sandbox.max_output_bytes {
                    let mut truncate_at = self.sandbox.max_output_bytes;
                    while truncate_at > 0 && !stdout.is_char_boundary(truncate_at) {
                        truncate_at -= 1;
                    }
                    stdout.truncate(truncate_at);
                    stdout.push_str("\n... [output truncated]");
                }
                if stderr.len() > self.sandbox.max_output_bytes {
                    let mut truncate_at = self.sandbox.max_output_bytes;
                    while truncate_at > 0 && !stderr.is_char_boundary(truncate_at) {
                        truncate_at -= 1;
                    }
                    stderr.truncate(truncate_at);
                    stderr.push_str("\n... [output truncated]");
                }

                Ok(ExecutionResult {
                    stdout,
                    stderr,
                    exit_code: status.code().unwrap_or(-1),
                    execution_time_ms: elapsed,
                    timed_out: false,
                })
            }
            Ok(Err(e)) => Err(anyhow!("Process error: {}", e)),
            Err(_) => {
                // Timeout — kill the process
                let _ = child.kill().await;
                Ok(ExecutionResult {
                    stdout: String::new(),
                    stderr: format!("Execution timed out after {} seconds", self.sandbox.timeout_seconds),
                    exit_code: -1,
                    execution_time_ms: elapsed,
                    timed_out: true,
                })
            }
        }
    }

    /// Check if python3 is available on the system.
    pub async fn check_python_available() -> Result<String> {
        let output = Command::new("python3")
            .arg("--version")
            .output()
            .await
            .context("python3 not found on system")?;

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if version.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Ok(stderr) // Some systems output version to stderr
        } else {
            Ok(version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_check_python_available() {
        let result = PythonRunner::check_python_available().await;
        // Python3 should be available on most dev machines
        if let Ok(version) = result {
            assert!(version.contains("Python") || version.contains("python"));
        }
    }

    #[test]
    fn test_execution_result_serialization() {
        let result = ExecutionResult {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: 0,
            execution_time_ms: 100,
            timed_out: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"exitCode\":0"));
        assert!(json.contains("\"timedOut\":false"));
    }
}
