#!/bin/bash
# Mock ri1-cli for testing subprocess lifecycle
# Supports the CLI arguments used by the application
# Scenario can be set via TEST_SCENARIO env var or --test-scenario argument

# Parse arguments
PROMPT=""
STREAM=false
OLLAMA_MODEL=""
OLLAMA_ENDPOINT=""
VERBOSE=false
SCENARIO=""

while [[ $# -gt 0 ]]; do
    case $1 in
        gen)
            shift
            if [[ $1 == "text" ]]; then
                shift
            fi
            ;;
        --prompt)
            PROMPT="$2"
            shift 2
            ;;
        --stream)
            STREAM=true
            shift
            ;;
        --ollama-model)
            OLLAMA_MODEL="$2"
            shift 2
            ;;
        --ollama-endpoint)
            OLLAMA_ENDPOINT="$2"
            shift 2
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --test-scenario)
            SCENARIO="$2"
            shift 2
            ;;
        --help)
            echo "ri1-cli mock - supports gen text --prompt <prompt> --stream [--ollama-model <model>] [--ollama-endpoint <endpoint>] [--verbose] [--test-scenario <scenario>]"
            exit 0
            ;;
        *)
            shift
            ;;
    esac
done

# Check for test scenario via argument or environment variable
if [[ -z "$SCENARIO" ]]; then
    SCENARIO=${TEST_SCENARIO:-normal}
fi

case $SCENARIO in
    normal)
        # Normal completion - emit chunks then exit 0
        echo "Response chunk 1"
        echo "Response chunk 2"
        echo "Response chunk 3"
        exit 0
        ;;
    partial-fail)
        # Partial output followed by nonzero exit
        echo "Partial output"
        echo "More partial output"
        exit 1
        ;;
    slow-start)
        # Slow startup for cancellation testing
        sleep 5
        echo "Should not reach here"
        exit 0
        ;;
    immediate-fail)
        # Immediate failure
        echo "Error: immediate failure" >&2
        exit 1
        ;;
    *)
        echo "Unknown scenario: $SCENARIO" >&2
        exit 1
        ;;
esac
