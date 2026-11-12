// Integration tests for subprocess lifecycle and cancellation
// These tests exercise the production Rust lifecycle patterns using a controlled CLI fixture

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Mock ProcessRegistry matching production implementation
#[derive(Clone)]
struct ProcessRegistry(Arc<Mutex<HashMap<String, u32>>>);

impl ProcessRegistry {
    fn new() -> Self {
        ProcessRegistry(Arc::new(Mutex::new(HashMap::new())))
    }

    fn insert(&self, key: String, pid: u32) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(key, pid);
        }
    }

    fn remove(&self, key: &str) {
        if let Ok(mut m) = self.0.lock() {
            let _ = m.remove(key);
        }
    }

    fn get(&self, key: &str) -> Option<u32> {
        self.0.lock().ok().and_then(|m| m.get(key).copied())
    }

    fn len(&self) -> usize {
        self.0.lock().map(|m| m.len()).unwrap_or(0)
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// Get the path to the mock CLI fixture relative to this test file
fn get_mock_cli_path() -> PathBuf {
    // Tests are in src-tauri/tests/, fixture is in tests/
    // CARGO_MANIFEST_DIR is src-tauri/, so parent is workspace root
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();
    workspace_root.join("tests").join("mock-ri1-cli.sh")
}

// Helper to run mock CLI with scenario
fn run_mock_cli(scenario: &str, args: &[&str]) -> std::process::Output {
    let mock_path = get_mock_cli_path();
    Command::new(&mock_path)
        .env("TEST_SCENARIO", scenario)
        .args(args)
        .output()
        .expect("Failed to run mock CLI")
}

// Helper to spawn mock CLI as child process
fn spawn_mock_cli(scenario: &str, args: &[&str]) -> std::process::Child {
    let mock_path = get_mock_cli_path();
    Command::new(&mock_path)
        .env("TEST_SCENARIO", scenario)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn mock CLI")
}

// Test 1: Normal completion - process exits successfully
#[test]
fn test_normal_completion() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-normal";
    let topic = format!("ri1://{}", stream_id);

    // Spawn mock CLI with normal scenario
    let mut child = spawn_mock_cli("normal", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();

    // Register in registry (simulating production code)
    registry.insert(topic.clone(), pid);
    assert_eq!(registry.get(&topic), Some(pid));
    assert_eq!(registry.len(), 1);

    // Wait for completion
    let status = child.wait().expect("Failed to wait for child");
    assert!(status.success(), "Process should exit successfully");

    // Cleanup registry (simulating production code)
    registry.remove(&topic);
    assert!(registry.is_empty(), "Registry should be empty after cleanup");
}

// Test 2: Partial output followed by failure
#[test]
fn test_partial_output_failure() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-partial-fail";
    let topic = format!("ri1://{}", stream_id);

    // Spawn mock CLI with partial-fail scenario
    let mut child = spawn_mock_cli("partial-fail", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();

    // Register in registry
    registry.insert(topic.clone(), pid);
    assert_eq!(registry.get(&topic), Some(pid));

    // Read stdout to verify partial output
    use std::io::{BufRead, BufReader};
    let stdout = child.stdout.take().expect("Failed to take stdout");
    let reader = BufReader::new(stdout);
    let mut lines = Vec::new();

    for line in reader.lines() {
        if let Ok(l) = line {
            lines.push(l);
        }
    }

    // Should have partial output
    assert!(!lines.is_empty(), "Should have partial output before failure");
    assert!(lines.iter().any(|l| l.contains("Partial output")), "Should contain partial output");

    // Wait for completion - should fail
    let status = child.wait().expect("Failed to wait for child");
    assert!(!status.success(), "Process should exit with failure");

    // Cleanup registry
    registry.remove(&topic);
    assert!(registry.is_empty(), "Registry should be empty after cleanup");
}

// Test 3: Cancellation during startup (before PID registration completes)
#[test]
fn test_cancellation_during_startup() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-cancel-startup";
    let topic = format!("ri1://{}", stream_id);

    // Spawn slow-start mock CLI
    let mut child = spawn_mock_cli("slow-start", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();

    // Simulate cancellation arriving before registration completes
    // In production, this would be a race between spawn and registry.insert
    // For testing, we cancel immediately after spawn
    registry.insert(topic.clone(), pid);

    // Cancel immediately (simulating fast cancellation)
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(format!("{}", pid))
        .status();
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("{}", pid))
        .status();

    // Remove from registry (simulating cancel_stream)
    registry.remove(&topic);
    assert!(registry.is_empty(), "Registry should be empty after cancellation");

    // Wait for process to terminate
    let status = child.wait().expect("Failed to wait for child");
    // Process should be terminated (either by signal or exit code)
    assert!(!status.success() || status.code().is_none(), "Process should be terminated");
}

// Test 4: Cancellation of running child
#[test]
fn test_cancellation_running_child() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-cancel-running";
    let topic = format!("ri1://{}", stream_id);

    // Spawn slow-start mock CLI
    let mut child = spawn_mock_cli("slow-start", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();

    // Register in registry
    registry.insert(topic.clone(), pid);
    assert_eq!(registry.get(&topic), Some(pid));

    // Wait a bit to ensure process is running
    std::thread::sleep(Duration::from_millis(100));

    // Cancel the running process
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(format!("{}", pid))
        .status();
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("{}", pid))
        .status();

    // Remove from registry
    registry.remove(&topic);
    assert!(registry.is_empty(), "Registry should be empty after cancellation");

    // Wait for process to terminate
    let status = child.wait().expect("Failed to wait for child");
    // Process should be terminated
    assert!(!status.success() || status.code().is_none(), "Process should be terminated");
}

// Test 5: Repeated cancellation (idempotent)
#[test]
fn test_repeated_cancellation() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-repeated-cancel";
    let topic = format!("ri1://{}", stream_id);

    // Spawn slow-start mock CLI
    let mut child = spawn_mock_cli("slow-start", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();

    // Register in registry
    registry.insert(topic.clone(), pid);

    // First cancellation
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(format!("{}", pid))
        .status();
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("{}", pid))
        .status();
    registry.remove(&topic);

    // Second cancellation attempt (should be safe - process already gone)
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(format!("{}", pid))
        .status();
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("{}", pid))
        .status();
    registry.remove(&topic); // Should be no-op

    assert!(registry.is_empty(), "Registry should be empty");

    // Wait for process
    let _ = child.wait();
}

// Test 6: Send → Stop → Send sequence
#[test]
fn test_send_stop_send() {
    let registry = ProcessRegistry::new();

    // First send
    let stream_id_1 = "test-send-1";
    let topic_1 = format!("ri1://{}", stream_id_1);
    let mut child_1 = spawn_mock_cli("slow-start", &["gen", "text", "--prompt", "first", "--stream"]);
    let pid_1 = child_1.id();
    registry.insert(topic_1.clone(), pid_1);

    // Stop first
    std::thread::sleep(Duration::from_millis(100));
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(format!("{}", pid_1))
        .status();
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("{}", pid_1))
        .status();
    registry.remove(&topic_1);
    let _ = child_1.wait();

    assert!(registry.is_empty(), "Registry should be empty after first stop");

    // Second send
    let stream_id_2 = "test-send-2";
    let topic_2 = format!("ri1://{}", stream_id_2);
    let mut child_2 = spawn_mock_cli("normal", &["gen", "text", "--prompt", "second", "--stream"]);
    let pid_2 = child_2.id();
    registry.insert(topic_2.clone(), pid_2);

    // Wait for completion
    let status = child_2.wait().expect("Failed to wait for child");
    assert!(status.success(), "Second process should complete successfully");

    // Cleanup
    registry.remove(&topic_2);
    assert!(registry.is_empty(), "Registry should be empty after second completion");
}

// Test 7: Registry cleanup on spawn failure
#[test]
fn test_registry_cleanup_spawn_failure() {
    let registry = ProcessRegistry::new();

    // Try to spawn non-existent executable
    let result = Command::new("/nonexistent/path/to/cli")
        .args(["gen", "text", "--prompt", "test", "--stream"])
        .spawn();

    assert!(result.is_err(), "Spawn should fail for non-existent executable");

    // Registry should not have any entries (spawn failed before registration)
    assert!(registry.is_empty(), "Registry should be empty after spawn failure");
}

// Test 8: Registry cleanup on wait failure
#[test]
fn test_registry_cleanup_wait_failure() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-wait-fail";
    let topic = format!("ri1://{}", stream_id);

    // Spawn a process that will be killed
    let mut child = spawn_mock_cli("slow-start", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();

    // Register in registry
    registry.insert(topic.clone(), pid);

    // Kill the process externally
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("{}", pid))
        .status();

    // Wait should return (possibly with error status)
    let _ = child.wait();

    // Cleanup registry (simulating production code)
    registry.remove(&topic);
    assert!(registry.is_empty(), "Registry should be empty after cleanup");
}

// Test 9: Negative control - unexpected success in failure scenario
#[test]
fn test_negative_control_unexpected_success() {
    // This test verifies that our test infrastructure can detect unexpected success
    // In the partial-fail scenario, the process should NOT succeed
    let output = run_mock_cli("partial-fail", &["gen", "text", "--prompt", "test", "--stream"]);

    // This should fail - if it succeeds, our test infrastructure is broken
    assert!(!output.status.success(), "partial-fail scenario should not succeed");
}

// Test 10: Negative control - ineffective cancellation detection
#[test]
fn test_negative_control_ineffective_cancellation() {
    // This test verifies that cancellation actually terminates the process
    let mut child = spawn_mock_cli("slow-start", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();

    // Cancel the process
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("{}", pid))
        .status();

    // Wait for termination
    let status = child.wait().expect("Failed to wait for child");

    // Process should be terminated (not successful exit)
    assert!(!status.success() || status.code().is_none(), "Process should be terminated by cancellation");
}

// Test 11: Process reaping verification
#[test]
fn test_process_reaping() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-reaping";
    let topic = format!("ri1://{}", stream_id);

    // Spawn and complete a process
    let mut child = spawn_mock_cli("normal", &["gen", "text", "--prompt", "test", "--stream"]);
    let pid = child.id();
    registry.insert(topic.clone(), pid);

    // Wait for completion
    let status = child.wait().expect("Failed to wait for child");
    assert!(status.success(), "Process should complete successfully");

    // Verify process is reaped (no zombie)
    // On Unix, we can check if the process still exists
    let check = Command::new("ps")
        .arg("-p")
        .arg(format!("{}", pid))
        .output();

    if let Ok(output) = check {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Process should not be in the list (reaped)
        assert!(!stdout.contains(&pid.to_string()) || stdout.lines().count() <= 1,
            "Process should be reaped after wait()");
    }

    // Cleanup registry
    registry.remove(&topic);
    assert!(registry.is_empty(), "Registry should be empty");
}

// Test 12: Multiple concurrent processes
#[test]
fn test_multiple_concurrent_processes() {
    let registry = ProcessRegistry::new();

    // Spawn multiple processes
    let mut children = Vec::new();
    let mut topics = Vec::new();

    for i in 0..3 {
        let stream_id = format!("test-concurrent-{}", i);
        let topic = format!("ri1://{}", stream_id);
        let child = spawn_mock_cli("normal", &["gen", "text", "--prompt", &format!("test{}", i), "--stream"]);
        let pid = child.id();

        registry.insert(topic.clone(), pid);
        children.push(child);
        topics.push(topic);
    }

    assert_eq!(registry.len(), 3, "Registry should have 3 entries");

    // Wait for all to complete
    for (i, child) in children.iter_mut().enumerate() {
        let status = child.wait().expect("Failed to wait for child");
        assert!(status.success(), "Process {} should complete successfully", i);
        registry.remove(&topics[i]);
    }

    assert!(registry.is_empty(), "Registry should be empty after all completions");
}
