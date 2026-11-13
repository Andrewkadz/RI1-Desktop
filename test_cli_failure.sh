#!/bin/bash
# Test CLI failure when stream ends before done:true

# Use a fixed port for the mock server
PORT=18923

# Start a mock server that sends incomplete stream
python3 -c "
import http.server
import socketserver
import json
import time

class MockHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path == '/api/generate':
            # Read request body
            content_length = int(self.headers.get('Content-Length', 0))
            self.rfile.read(content_length)
            
            # Prepare response
            responses = [
                {'response': 'Hello', 'done': False},
                {'response': ' World', 'done': False},
            ]
            response_body = ''.join([json.dumps(r) + '\n' for r in responses])
            
            self.send_response(200)
            self.send_header('Content-Type', 'application/x-ndjson')
            self.send_header('Content-Length', str(len(response_body)))
            self.end_headers()
            
            # Send response in chunks to simulate streaming
            for i, resp in enumerate(responses):
                chunk = json.dumps(resp) + '\n'
                self.wfile.write(chunk.encode())
                self.wfile.flush()
                time.sleep(0.1)
        else:
            self.send_error(404)
    
    def log_message(self, format, *args):
        pass

with socketserver.TCPServer(('127.0.0.1', $PORT), MockHandler) as httpd:
    print(f'Mock server running on port $PORT', flush=True)
    httpd.serve_forever()
" &

MOCK_PID=$!
sleep 1

echo "Mock server running on port $PORT"

# Run the CLI with streaming enabled
cd /Users/andykadziolka/Downloads/ri1-desktop/src-tauri
echo "Running CLI with incomplete stream..."
./target/debug/ri1-cli gen text --prompt "test" --stream --ollama-model "test-model" --ollama-endpoint "http://127.0.0.1:$PORT" 2>&1
EXIT_CODE=$?

echo "CLI exit code: $EXIT_CODE"

# Cleanup
kill $MOCK_PID 2>/dev/null

# Check if exit code is nonzero
if [ $EXIT_CODE -eq 0 ]; then
    echo "FAIL: CLI should have exited with nonzero code"
    exit 1
else
    echo "PASS: CLI exited with nonzero code as expected"
fi
