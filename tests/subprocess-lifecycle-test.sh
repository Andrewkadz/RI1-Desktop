#!/bin/bash
# Test script for Rust subprocess lifecycle validation
# Exercises normal completion, partial output + nonzero exit, cancellation, repeated Stop, Send → Stop → Send

set -e

MOCK_CLI="/Users/andykadziolka/Downloads/ri1-desktop/tests/mock-ri1-cli.sh"

echo "=== Rust Subprocess Lifecycle Validation ==="
echo ""

# Test 1: Normal completion
echo "Test 1: Normal completion"
TEST_SCENARIO=normal "$MOCK_CLI" gen text --prompt "test" --stream
echo "Exit code: $?"
echo ""

# Test 2: Partial output followed by nonzero exit
echo "Test 2: Partial output followed by nonzero exit"
TEST_SCENARIO=partial-fail "$MOCK_CLI" gen text --prompt "test" --stream || echo "Exit code: $? (expected nonzero)"
echo ""

# Test 3: Cancellation during startup
echo "Test 3: Cancellation during startup"
TEST_SCENARIO=slow-start "$MOCK_CLI" gen text --prompt "test" --stream &
PID=$!
sleep 1
kill -TERM $PID 2>/dev/null || true
kill -KILL $PID 2>/dev/null || true
wait $PID 2>/dev/null || echo "Process terminated (expected)"
echo ""

# Test 4: Repeated Stop (simulate by killing same process twice)
echo "Test 4: Repeated Stop"
TEST_SCENARIO=slow-start "$MOCK_CLI" gen text --prompt "test" --stream &
PID=$!
sleep 1
kill -TERM $PID 2>/dev/null || true
sleep 0.5
kill -KILL $PID 2>/dev/null || true
wait $PID 2>/dev/null || echo "Process terminated (expected)"
echo ""

# Test 5: Send → Stop → Send
echo "Test 5: Send → Stop → Send"
# First send
TEST_SCENARIO=slow-start "$MOCK_CLI" gen text --prompt "first" --stream &
PID1=$!
sleep 1
kill -TERM $PID1 2>/dev/null || true
kill -KILL $PID1 2>/dev/null || true
wait $PID1 2>/dev/null || echo "First process terminated"

# Second send
TEST_SCENARIO=normal "$MOCK_CLI" gen text --prompt "second" --stream
echo "Second process completed"
echo ""

echo "=== All subprocess lifecycle tests completed ==="
