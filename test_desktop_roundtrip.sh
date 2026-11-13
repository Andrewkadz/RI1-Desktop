#!/bin/bash
# Test desktop round trip with mock Ollama server

# Use a fixed port for the mock server
PORT=18924
CLI_PATH="/Users/andykadziolka/Downloads/ri1-desktop/src-tauri/target/debug/ri1-cli"

# Start a mock Ollama server that returns a specific response
python3 -c "
import http.server
import socketserver
import json
import time

request_count = 0

class MockHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        global request_count
        if self.path == '/api/generate':
            request_count += 1
            print(f'Request #{request_count} received', flush=True)
            
            # Read request body
            content_length = int(self.headers.get('Content-Length', 0))
            body = self.rfile.read(content_length)
            print(f'Request body: {body.decode()}', flush=True)
            
            # Prepare response with marker that wasn't in prompt
            response_text = 'Φπε-731'
            responses = [
                {'response': response_text, 'done': True}
            ]
            response_body = ''.join([json.dumps(r) + '\n' for r in responses])
            
            self.send_response(200)
            self.send_header('Content-Type', 'application/x-ndjson')
            self.send_header('Content-Length', str(len(response_body)))
            self.end_headers()
            
            # Send response
            for resp in responses:
                chunk = json.dumps(resp) + '\n'
                self.wfile.write(chunk.encode())
                self.wfile.flush()
                time.sleep(0.1)
        else:
            self.send_error(404)
    
    def log_message(self, format, *args):
        pass

with socketserver.TCPServer(('127.0.0.1', $PORT), MockHandler) as httpd:
    print(f'Mock Ollama server running on port $PORT', flush=True)
    httpd.serve_forever()
" &

MOCK_PID=$!
sleep 1

echo "Mock Ollama server running on port $PORT"
echo "CLI path: $CLI_PATH"

# Test the CLI directly first
cd /Users/andykadziolka/Downloads/ri1-desktop/src-tauri
echo "Testing CLI directly..."
$CLI_PATH gen text --prompt "test prompt" --stream --ollama-model "test-model" --ollama-endpoint "http://127.0.0.1:$PORT" 2>&1
CLI_EXIT=$?

echo "CLI exit code: $CLI_EXIT"

# Cleanup
kill $MOCK_PID 2>/dev/null

if [ $CLI_EXIT -eq 0 ]; then
    echo "PASS: CLI completed successfully"
else
    echo "FAIL: CLI exited with code $CLI_EXIT"
    exit 1
fi
