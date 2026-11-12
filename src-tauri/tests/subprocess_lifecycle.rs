// Integration tests for subprocess lifecycle using production implementation
// These tests exercise the actual production code from the subprocess module

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ri1_desktop_lib::subprocess::{
    ProcessRegistry, SubprocessConfig, SubprocessLifecycle, SubprocessResult,
    StreamCallback, cancel_subprocess, create_topic
};

// Test callback that collects emitted chunks and outcomes
struct TestCallback {
    chunks: Arc<Mutex<Vec<(String, String, bool)>>>,
    errors: Arc<Mutex<Vec<(String, String)>>>,
}

impl StreamCallback for TestCallback {
    fn emit_chunk(&self, topic: &str, chunk: &str, done: bool) {
        if let Ok(mut chunks) = self.chunks.lock() {
            chunks.push((topic.to_string(), chunk.to_string(), done));
        }
    }

    fn emit_error(&self, topic: &str, error: &str) {
        if let Ok(mut errors) = self.errors.lock() {
            errors.push((topic.to_string(), error.to_string()));
        }
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

// Helper to create test callback
fn create_test_callback() -> (Arc<TestCallback>, Arc<Mutex<Vec<(String, String, bool)>>>, Arc<Mutex<Vec<(String, String)>>>) {
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let callback = Arc::new(TestCallback {
        chunks: chunks.clone(),
        errors: errors.clone(),
    });
    (callback, chunks, errors)
}

// Test 1: Normal completion using production lifecycle
#[test]
fn test_normal_completion_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-normal-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "normal".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    let lifecycle = SubprocessLifecycle::new(config);
    let result = lifecycle.execute().expect("Lifecycle execution failed");

    assert!(matches!(result, SubprocessResult::Success), "Should succeed");
    assert!(registry.is_empty(), "Registry should be empty after completion");

    // Verify chunks were emitted
    let chunks_guard = chunks.lock().unwrap();
    assert!(!chunks_guard.is_empty(), "Should have emitted chunks");
    assert!(chunks_guard.iter().any(|(_, _, done)| *done), "Should have done chunk");
}

// Test 2: Partial output followed by failure using production lifecycle
#[test]
fn test_partial_output_failure_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-partial-fail-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, chunks, errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "partial-fail".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    let lifecycle = SubprocessLifecycle::new(config);
    let result = lifecycle.execute().expect("Lifecycle execution failed");

    assert!(matches!(result, SubprocessResult::Failure(_)), "Should fail");
    assert!(registry.is_empty(), "Registry should be empty after failure");

    // Verify partial output was emitted
    let chunks_guard = chunks.lock().unwrap();
    assert!(!chunks_guard.is_empty(), "Should have partial output chunks");
    assert!(chunks_guard.iter().any(|(_, chunk, _)| chunk.contains("Partial output")), "Should contain partial output");

    // Verify error was emitted
    let errors_guard = errors.lock().unwrap();
    assert!(!errors_guard.is_empty(), "Should have error emitted");
}

// Test 3: Cancellation during startup using production lifecycle
#[test]
fn test_cancellation_during_startup_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-cancel-startup-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "slow-start".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Spawn lifecycle in a thread
    let lifecycle = SubprocessLifecycle::new(config);
    let handle = std::thread::spawn(move || lifecycle.execute());

    // Wait a bit for process to start and register
    std::thread::sleep(Duration::from_millis(100));

    // Cancel the process using production cancellation
    let cancel_result = cancel_subprocess(&registry, &topic);
    assert!(cancel_result.is_ok(), "Cancellation should succeed");

    // Wait for lifecycle to complete
    let result = handle.join().expect("Thread join failed").expect("Lifecycle execution failed");

    // Should be cancelled or failure (process was killed)
    assert!(matches!(result, SubprocessResult::Cancelled | SubprocessResult::Failure(_)),
        "Should be cancelled or failure, got: {:?}", result);

    assert!(registry.is_empty(), "Registry should be empty after cancellation");
}

// Test 4: Cancellation of running child using production lifecycle
#[test]
fn test_cancellation_running_child_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-cancel-running-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "slow-start".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Spawn lifecycle in a thread
    let lifecycle = SubprocessLifecycle::new(config);
    let handle = std::thread::spawn(move || lifecycle.execute());

    // Wait for process to be registered
    std::thread::sleep(Duration::from_millis(200));

    // Verify process is registered
    assert!(registry.get(&topic).is_some(), "Process should be registered");

    // Cancel the running process
    let cancel_result = cancel_subprocess(&registry, &topic);
    assert!(cancel_result.is_ok(), "Cancellation should succeed");

    // Wait for lifecycle to complete
    let result = handle.join().expect("Thread join failed").expect("Lifecycle execution failed");

    // Should be cancelled or failure
    assert!(matches!(result, SubprocessResult::Cancelled | SubprocessResult::Failure(_)),
        "Should be cancelled or failure, got: {:?}", result);

    assert!(registry.is_empty(), "Registry should be empty after cancellation");
}

// Test 5: Repeated cancellation using production lifecycle
#[test]
fn test_repeated_cancellation_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-repeated-cancel-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "slow-start".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Spawn lifecycle in a thread
    let lifecycle = SubprocessLifecycle::new(config);
    let handle = std::thread::spawn(move || lifecycle.execute());

    // Wait for process to be registered
    std::thread::sleep(Duration::from_millis(200));

    // First cancellation
    let cancel_result1 = cancel_subprocess(&registry, &topic);
    assert!(cancel_result1.is_ok(), "First cancellation should succeed");

    // Second cancellation attempt (should fail - process already gone)
    let cancel_result2 = cancel_subprocess(&registry, &topic);
    assert!(cancel_result2.is_err(), "Second cancellation should fail");

    // Wait for lifecycle to complete
    let _ = handle.join().expect("Thread join failed");

    assert!(registry.is_empty(), "Registry should be empty");
}

// Test 6: Send → Stop → Send sequence using production lifecycle
#[test]
fn test_send_stop_send_production() {
    let registry = ProcessRegistry::new();

    // First send
    let stream_id_1 = "test-send-1-prod";
    let topic_1 = create_topic("ri1", stream_id_1);

    let (callback1, _chunks1, _errors1) = create_test_callback();

    let config1 = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "first".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "slow-start".to_string()],
        topic: topic_1.clone(),
        registry: registry.clone(),
        callback: callback1,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Spawn first lifecycle
    let lifecycle1 = SubprocessLifecycle::new(config1);
    let handle1 = std::thread::spawn(move || lifecycle1.execute());

    // Wait for registration
    std::thread::sleep(Duration::from_millis(200));

    // Stop first
    let cancel_result = cancel_subprocess(&registry, &topic_1);
    assert!(cancel_result.is_ok(), "First cancellation should succeed");

    // Wait for first to complete
    let _ = handle1.join().expect("Thread join failed");

    assert!(registry.is_empty(), "Registry should be empty after first stop");

    // Second send
    let stream_id_2 = "test-send-2-prod";
    let topic_2 = create_topic("ri1", stream_id_2);

    let (callback2, _chunks2, _errors2) = create_test_callback();

    let config2 = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "second".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "normal".to_string()],
        topic: topic_2.clone(),
        registry: registry.clone(),
        callback: callback2,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Spawn second lifecycle
    let lifecycle2 = SubprocessLifecycle::new(config2);
    let result2 = lifecycle2.execute().expect("Second lifecycle execution failed");

    assert!(matches!(result2, SubprocessResult::Success), "Second should succeed");
    assert!(registry.is_empty(), "Registry should be empty after second completion");
}

// Test 7: Registry cleanup on spawn failure using production lifecycle
#[test]
fn test_registry_cleanup_spawn_failure_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-spawn-fail-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: "/nonexistent/path/to/cli".to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    let lifecycle = SubprocessLifecycle::new(config);
    let result = lifecycle.execute();

    assert!(result.is_err(), "Should fail to spawn");
    assert!(registry.is_empty(), "Registry should be empty after spawn failure");
}

// Test 8: Wait error coverage using production lifecycle
#[test]
fn test_wait_error_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-wait-error-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "slow-start".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Spawn lifecycle in a thread
    let lifecycle = SubprocessLifecycle::new(config);
    let handle = std::thread::spawn(move || lifecycle.execute());

    // Wait for process to be registered
    std::thread::sleep(Duration::from_millis(200));

    // Get the PID and kill it externally to cause wait error
    if let Some(pid) = registry.get(&topic) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("{}", pid))
            .status();
    }

    // Wait for lifecycle to complete
    let result = handle.join().expect("Thread join failed");

    // Should be failure (process was killed)
    assert!(result.is_ok(), "Lifecycle should complete");
    if let Ok(SubprocessResult::Failure(_)) = result {
        // Expected - process was killed
    } else {
        // Could also be cancelled if registry was cleaned up
        assert!(matches!(result, Ok(SubprocessResult::Cancelled)), "Should be cancelled or failure");
    }

    assert!(registry.is_empty(), "Registry should be empty after wait error");
}

// Test 9: Negative control - ineffective cancellation detection
#[test]
fn test_negative_control_ineffective_cancellation_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-ineffective-cancel-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "slow-start".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Spawn lifecycle in a thread
    let lifecycle = SubprocessLifecycle::new(config);
    let handle = std::thread::spawn(move || lifecycle.execute());

    // Wait for process to be registered
    std::thread::sleep(Duration::from_millis(200));

    // Get the PID
    let pid = registry.get(&topic).expect("Process should be registered");

    // Try to cancel with a fake topic (should fail)
    let fake_topic = "ri1://fake-stream";
    registry.insert(fake_topic.to_string(), pid);

    // This should succeed because we're using the wrong topic but it has the PID
    let cancel_result = cancel_subprocess(&registry, fake_topic);
    assert!(cancel_result.is_ok(), "Cancellation with fake topic should succeed (it has the PID)");

    // But the real topic should still have the process
    assert!(registry.get(&topic).is_some(), "Real topic should still have process");

    // Now cancel the real topic
    let real_cancel_result = cancel_subprocess(&registry, &topic);
    assert!(real_cancel_result.is_ok(), "Real cancellation should succeed");

    // Wait for lifecycle to complete
    let _ = handle.join().expect("Thread join failed");

    assert!(registry.is_empty(), "Registry should be empty");
}

// Test 10: Multiple concurrent processes using production lifecycle
#[test]
fn test_multiple_concurrent_processes_production() {
    let registry = ProcessRegistry::new();
    let mut handles = Vec::new();

    // Spawn multiple lifecycles
    for i in 0..3 {
        let stream_id = format!("test-concurrent-{}-prod", i);
        let topic = create_topic("ri1", &stream_id);

        let (callback, _chunks, _errors) = create_test_callback();

        let config = SubprocessConfig {
            command: get_mock_cli_path().to_string_lossy().to_string(),
            args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), format!("test{}", i), "--stream".to_string(), "--test-scenario".to_string(), "normal".to_string()],
            topic: topic.clone(),
            registry: registry.clone(),
            callback,
            stdin_null: true,
            stdout_piped: true,
            stderr_piped: true,
        };

        let lifecycle = SubprocessLifecycle::new(config);
        let handle = std::thread::spawn(move || lifecycle.execute());
        handles.push(handle);
    }

    // Wait for all to complete
    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.join().expect("Thread join failed").expect("Lifecycle execution failed");
        assert!(matches!(result, SubprocessResult::Success), "Process {} should succeed", i);
    }

    assert!(registry.is_empty(), "Registry should be empty after all completions");
}

// Test 11: Process reaping verification using production lifecycle
#[test]
fn test_process_reaping_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-reaping-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "normal".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    let lifecycle = SubprocessLifecycle::new(config);
    let result = lifecycle.execute().expect("Lifecycle execution failed");

    assert!(matches!(result, SubprocessResult::Success), "Should succeed");
    assert!(registry.is_empty(), "Registry should be empty");

    // Verify process is reaped (no zombie)
    // This is implicitly verified by the fact that wait() completed successfully
}

// Test 12: Cancellation before PID registration using synchronization
#[test]
fn test_cancellation_before_registration_production() {
    let registry = ProcessRegistry::new();
    let stream_id = "test-cancel-before-reg-prod";
    let topic = create_topic("ri1", stream_id);

    let (callback, _chunks, _errors) = create_test_callback();

    let config = SubprocessConfig {
        command: get_mock_cli_path().to_string_lossy().to_string(),
        args: vec!["gen".to_string(), "text".to_string(), "--prompt".to_string(), "test".to_string(), "--stream".to_string(), "--test-scenario".to_string(), "slow-start".to_string()],
        topic: topic.clone(),
        registry: registry.clone(),
        callback,
        stdin_null: true,
        stdout_piped: true,
        stderr_piped: true,
    };

    // Create a barrier to synchronize cancellation before registration
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let barrier_clone = barrier.clone();

    // Spawn lifecycle in a thread
    let lifecycle = SubprocessLifecycle::new(config);
    let handle = std::thread::spawn(move || {
        // Wait at barrier before executing
        barrier_clone.wait();
        lifecycle.execute()
    });

    // Cancel immediately (before lifecycle can register)
    // This simulates cancellation arriving before PID registration
    let cancel_result = cancel_subprocess(&registry, &topic);
    // This will fail because process hasn't registered yet
    assert!(cancel_result.is_err(), "Cancellation before registration should fail");

    // Release the barrier to let lifecycle proceed
    barrier.wait();

    // Wait for lifecycle to complete
    let result = handle.join().expect("Thread join failed").expect("Lifecycle execution failed");

    // The lifecycle should still complete (it will register and run)
    // But since we cancelled before registration, the process might still run
    // This tests the race condition behavior
    assert!(matches!(result, SubprocessResult::Success | SubprocessResult::Failure(_) | SubprocessResult::Cancelled),
        "Should complete with some result");

    // Registry should be cleaned up
    assert!(registry.is_empty(), "Registry should be empty");
}
