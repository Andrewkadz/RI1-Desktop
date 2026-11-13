use ri1_core::modality::{GenerationRequest, GenerationResponse, Modality};
use reqwest::Client;
use serde_json;
use std::io::Write;

pub struct OllamaText {
    model: String,
    endpoint: String,
    stream: bool,
}

impl OllamaText {
    pub fn new(model: String, endpoint: String, stream: bool) -> Self {
        Self { model, endpoint, stream }
    }
}

impl Modality for OllamaText {
    fn name(&self) -> &'static str {
        "text"
    }

    fn generate(&self, req: GenerationRequest) -> GenerationResponse {
        // Use futures::executor::block_on to avoid tokio runtime conflicts
        futures::executor::block_on(async {
            self.generate_async(req).await
        })
    }
}

impl OllamaText {
    async fn generate_async(&self, req: GenerationRequest) -> GenerationResponse {
        let client = Client::new();
        let url = format!("{}/api/generate", self.endpoint);
        
        let request_body = serde_json::json!({
            "model": self.model,
            "prompt": req.prompt,
            "stream": self.stream
        });

        let response = client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .expect("Failed to send request to Ollama");

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            panic!("Ollama request failed with status {}: {}", status, body);
        }

        if self.stream {
            // Handle streaming response
            let mut stream = response.bytes_stream();
            let mut buffer = Vec::new();
            let mut content = String::new();
            let mut done = false;

            use futures::StreamExt;
            
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.expect("Error reading stream");
                buffer.extend_from_slice(&chunk);

                // Process complete lines
                while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_bytes = buffer[..pos].to_vec();
                    buffer = buffer[pos + 1..].to_vec();

                    if !line_bytes.is_empty() {
                        match serde_json::from_slice::<serde_json::Value>(&line_bytes) {
                            Ok(json) => {
                                // Check for error field first
                                if let Some(error) = json.get("error").and_then(|v| v.as_str()) {
                                    eprintln!("Ollama error: {}", error);
                                    std::process::exit(1);
                                }

                                // Check for response field
                                if let Some(response_text) = json.get("response").and_then(|v| v.as_str()) {
                                    print!("{}", response_text);
                                    std::io::stdout().flush().ok();
                                    content.push_str(response_text);
                                }

                                // Check for done field
                                if let Some(done_val) = json.get("done").and_then(|v| v.as_bool()) {
                                    if done_val {
                                        done = true;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Malformed JSON in stream: {}", e);
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }

            // Process any remaining buffer at EOF
            if !buffer.is_empty() {
                match serde_json::from_slice::<serde_json::Value>(&buffer) {
                    Ok(json) => {
                        // Check for error field first
                        if let Some(error) = json.get("error").and_then(|v| v.as_str()) {
                            eprintln!("Ollama error: {}", error);
                            std::process::exit(1);
                        }

                        // Check for response field
                        if let Some(response_text) = json.get("response").and_then(|v| v.as_str()) {
                            print!("{}", response_text);
                            std::io::stdout().flush().ok();
                            content.push_str(response_text);
                        }

                        // Check for done field
                        if let Some(done_val) = json.get("done").and_then(|v| v.as_bool()) {
                            if done_val {
                                done = true;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Malformed JSON at EOF: {}", e);
                        std::process::exit(1);
                    }
                }
            }

            if !done {
                eprintln!("Stream ended without completion (done: true)");
                std::process::exit(1);
            }

            GenerationResponse { content }
        } else {
            // Handle non-streaming response
            let body = response.bytes().await.expect("Failed to read response");
            match serde_json::from_slice::<serde_json::Value>(&body) {
                Ok(json) => {
                    // Check for error field first
                    if let Some(error) = json.get("error").and_then(|v| v.as_str()) {
                        eprintln!("Ollama error: {}", error);
                        std::process::exit(1);
                    }

                    if let Some(response_text) = json.get("response").and_then(|v| v.as_str()) {
                        println!("{}", response_text);
                        GenerationResponse { content: response_text.to_string() }
                    } else {
                        eprintln!("Invalid response format from Ollama");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to parse Ollama response as JSON: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
