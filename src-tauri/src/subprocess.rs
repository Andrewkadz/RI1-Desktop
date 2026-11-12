// Shared subprocess lifecycle management for Tauri commands and tests
// This module provides the production implementation used by both the application and tests

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::process::{Child, Command, Stdio};
use std::io::{BufRead, BufReader};
use std::thread;

/// Process registry for tracking running subprocesses by stream key
#[derive(Clone)]
pub struct ProcessRegistry(Arc<Mutex<HashMap<String, u32>>>);

impl ProcessRegistry {
    pub fn new() -> Self {
        ProcessRegistry(Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn insert(&self, key: String, pid: u32) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(key, pid);
        }
    }

    pub fn remove(&self, key: &str) {
        if let Ok(mut m) = self.0.lock() {
            let _ = m.remove(key);
        }
    }

    pub fn get(&self, key: &str) -> Option<u32> {
        self.0.lock().ok().and_then(|m| m.get(key).copied())
    }

    pub fn len(&self) -> usize {
        self.0.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Callback for emitting stream chunks and terminal outcomes
/// This provides a testable boundary for event emission
pub trait StreamCallback: Send + Sync {
    fn emit_chunk(&self, topic: &str, chunk: &str, done: bool);
    fn emit_error(&self, topic: &str, error: &str);
}

/// No-op callback for production use (Tauri commands will implement this)
pub struct NoOpCallback;

impl StreamCallback for NoOpCallback {
    fn emit_chunk(&self, _topic: &str, _chunk: &str, _done: bool) {}
    fn emit_error(&self, _topic: &str, _error: &str) {}
}

/// Configuration for subprocess lifecycle
pub struct SubprocessConfig {
    pub command: String,
    pub args: Vec<String>,
    pub topic: String,
    pub registry: ProcessRegistry,
    pub callback: Arc<dyn StreamCallback>,
    pub stdin_null: bool,
    pub stdout_piped: bool,
    pub stderr_piped: bool,
}

/// Result of subprocess execution
#[derive(Debug)]
pub enum SubprocessResult {
    Success,
    Failure(String),
    Cancelled,
}

/// Manages subprocess lifecycle: spawn, register, stream, wait, cleanup
pub struct SubprocessLifecycle {
    config: SubprocessConfig,
}

impl SubprocessLifecycle {
    pub fn new(config: SubprocessConfig) -> Self {
        SubprocessLifecycle { config }
    }

    /// Execute the subprocess lifecycle
    /// This is the production implementation used by Tauri commands
    pub fn execute(&self) -> Result<SubprocessResult, String> {
        let topic = &self.config.topic;
        let registry = &self.config.registry;

        // Spawn the subprocess
        let mut child = self.spawn_child()?;

        // Register PID immediately after spawning
        let pid = child.id();
        registry.insert(topic.clone(), pid);

        // Check if already cancelled before we started streaming
        if registry.get(topic).is_none() {
            // Process was cancelled before registration completed
            let _ = child.kill();
            let _ = child.wait();
            return Ok(SubprocessResult::Cancelled);
        }

        // Stream stdout
        self.stream_stdout(&mut child)?;

        // Stream stderr (if piped)
        self.stream_stderr(&mut child)?;

        // Wait for completion
        let status = self.wait_for_completion(&mut child)?;

        // Cleanup registry
        registry.remove(topic);

        // Determine result
        if status.success() {
            self.config.callback.emit_chunk(topic, "", true);
            Ok(SubprocessResult::Success)
        } else {
            let error = format!("Process exited with status: {}", status);
            self.config.callback.emit_error(topic, &error);
            Ok(SubprocessResult::Failure(error))
        }
    }

    fn spawn_child(&self) -> Result<Child, String> {
        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args);

        if self.config.stdin_null {
            cmd.stdin(Stdio::null());
        }
        if self.config.stdout_piped {
            cmd.stdout(Stdio::piped());
        }
        if self.config.stderr_piped {
            cmd.stderr(Stdio::piped());
        }

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn {}: {}", self.config.command, e))
    }

    fn stream_stdout(&self, child: &mut Child) -> Result<(), String> {
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            let topic = self.config.topic.clone();
            let callback = self.config.callback.clone();

            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        // Emit line with space to preserve word boundaries
                        let chunk_with_space = if l.is_empty() { l } else { format!("{} ", l) };
                        callback.emit_chunk(&topic, &chunk_with_space, false);
                    }
                    Err(_) => break,
                }
            }
        }
        Ok(())
    }

    fn stream_stderr(&self, child: &mut Child) -> Result<(), String> {
        if let Some(stderr) = child.stderr.take() {
            let topic = self.config.topic.clone();
            let callback = self.config.callback.clone();

            let handle = thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(l) = line {
                        let msg = format!("[err] {}", l);
                        callback.emit_chunk(&topic, &msg, false);
                    } else {
                        break;
                    }
                }
            });

            // Wait for stderr thread to complete
            let _ = handle.join();
        }
        Ok(())
    }

    fn wait_for_completion(&self, child: &mut Child) -> Result<std::process::ExitStatus, String> {
        child.wait()
            .map_err(|e| format!("Wait error: {}", e))
    }
}

/// Cancellation function for subprocesses
pub fn cancel_subprocess(registry: &ProcessRegistry, topic: &str) -> Result<(), String> {
    if let Some(pid) = registry.get(topic) {
        // Try SIGTERM then SIGKILL
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("{}", pid))
            .status();
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("{}", pid))
            .status();
        registry.remove(topic);
        Ok(())
    } else {
        Err("No running process for stream".to_string())
    }
}

/// Helper to create stream topic from backend and stream_id
pub fn create_topic(backend: &str, stream_id: &str) -> String {
    match backend {
        "ollama" => format!("ollama://{}", stream_id),
        "kosmos" => format!("kosmos://{}", stream_id),
        _ => format!("ri1://{}", stream_id),
    }
}
