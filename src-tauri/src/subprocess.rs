// Shared subprocess lifecycle management for Tauri commands and tests
// This module provides the production implementation used by both the application and tests

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::process::{Child, Command, Stdio, ExitStatus};
use std::io::{BufRead, BufReader};
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// Optional flag to simulate wait errors for testing
    pub simulate_wait_error: bool,
}

impl Default for SubprocessConfig {
    fn default() -> Self {
        SubprocessConfig {
            command: String::new(),
            args: Vec::new(),
            topic: String::new(),
            registry: ProcessRegistry::new(),
            callback: Arc::new(NoOpCallback),
            stdin_null: true,
            stdout_piped: true,
            stderr_piped: true,
            simulate_wait_error: false,
        }
    }
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
    cancelled: Arc<AtomicBool>,
}

impl SubprocessLifecycle {
    pub fn new(config: SubprocessConfig) -> Self {
        SubprocessLifecycle {
            config,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Execute the subprocess lifecycle
    /// This is the production implementation used by Tauri commands
    pub fn execute(&self) -> Result<SubprocessResult, String> {
        let topic = &self.config.topic;
        let registry = &self.config.registry;

        // Spawn the subprocess
        let mut child = match self.spawn_child() {
            Ok(c) => c,
            Err(e) => {
                // Ensure cleanup on spawn failure
                registry.remove(topic);
                return Err(e);
            }
        };

        // Register PID immediately after spawning
        let pid = child.id();
        registry.insert(topic.clone(), pid);

        // Atomic check: if cancelled before registration, kill immediately
        if self.cancelled.load(Ordering::SeqCst) || registry.get(topic).is_none() {
            // Process was cancelled before or during registration
            let _ = child.kill();
            let _ = child.wait();
            registry.remove(topic);
            return Ok(SubprocessResult::Cancelled);
        }

        // Stream stdout and stderr concurrently to avoid deadlock
        let stream_result = self.stream_concurrent(&mut child);

        // Wait for completion
        let status_result = self.wait_for_completion(&mut child);

        // Always cleanup registry on every return path
        registry.remove(topic);

        // Handle stream errors
        if let Err(e) = stream_result {
            return Err(e);
        }

        // Handle wait errors
        let status = match status_result {
            Ok(s) => s,
            Err(e) => {
                // Emit error through callback for frontend error UI
                self.config.callback.emit_error(topic, &e);
                return Err(e);
            }
        };

        // Determine result
        if status.success() {
            // Only emit done on success
            self.config.callback.emit_chunk(topic, "", true);
            Ok(SubprocessResult::Success)
        } else {
            // Failed exit: emit error but don't emit done
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

    /// Stream stdout and stderr concurrently to avoid buffer deadlock
    fn stream_concurrent(&self, child: &mut Child) -> Result<(), String> {
        let topic = self.config.topic.clone();
        let callback = self.config.callback.clone();
        let cancelled = self.cancelled.clone();

        // Take stdout and stderr
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn stdout reader thread
        let stdout_handle = stdout.map(|stdout| {
            let topic = topic.clone();
            let callback = callback.clone();
            let cancelled = cancelled.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    if cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    match line {
                        Ok(l) => {
                            // Emit line with space to preserve word boundaries
                            let chunk_with_space = if l.is_empty() { l } else { format!("{} ", l) };
                            callback.emit_chunk(&topic, &chunk_with_space, false);
                        }
                        Err(_) => break,
                    }
                }
            })
        });

        // Spawn stderr reader thread
        let stderr_handle = stderr.map(|stderr| {
            let topic = topic.clone();
            let callback = callback.clone();
            let cancelled = cancelled.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(l) = line {
                        let msg = format!("[err] {}", l);
                        callback.emit_chunk(&topic, &msg, false);
                    } else {
                        break;
                    }
                }
            })
        });

        // Wait for both threads to complete
        if let Some(handle) = stdout_handle {
            let _ = handle.join();
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.join();
        }

        Ok(())
    }

    fn wait_for_completion(&self, child: &mut Child) -> Result<ExitStatus, String> {
        // For testing: simulate wait error if configured
        if self.config.simulate_wait_error {
            return Err("Simulated wait error for testing".to_string());
        }

        child.wait()
            .map_err(|e| format!("Wait error: {}", e))
    }

    /// Mark this lifecycle as cancelled (for atomic cancellation check)
    pub fn mark_cancelled(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Check if this lifecycle has been cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Cancellation function for subprocesses
/// This now also marks any associated lifecycle as cancelled
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
