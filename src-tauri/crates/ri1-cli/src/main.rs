use clap::{Parser, Subcommand};
use ri1_core::modality::GenerationRequest;
use ri1_core::Orchestrator;
use ri1_core::constraints::{ResonanceEvent, OperatorClass, ConstraintResult};
use ri1_symbolic_meta::{MetaEngineImpl, InfluenceSnapshot, compute_influence};
use ri1_text::BasicText;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use serde_json;
use uuid::Uuid;
use std::path::PathBuf;
use std::fs;
use reqwest::Client;
use futures::StreamExt;

#[derive(Parser, Debug)]
#[command(name = "ri1", version, about = "RI1 Hybrid Generative Engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate content
    Gen {
        #[command(subcommand)]
        modality: GenModality,
    },
}

#[derive(Subcommand, Debug)]
enum GenModality {
    /// Text generation
    Text {
        /// Prompt string
        #[arg(short, long)]
        prompt: String,
        /// Print resonance events
        #[arg(long, default_value_t = false)]
        verbose: bool,
        /// Output resonance events as JSON
        #[arg(long, default_value_t = false)]
        json: bool,
        /// Show influence block in verbose output
        #[arg(long, default_value_t = false)]
        influence: bool,
        /// Optional correlation id (uuid); if not provided, generated per run
        #[arg(long)]
        cid: Option<String>,
        /// Write a JSON envelope (with cid, content, constraints, events) to file
        #[arg(long)]
        log_file: Option<PathBuf>,
        /// Stream output line by line
        #[arg(long, default_value_t = false)]
        stream: bool,
        /// Ollama model to use
        #[arg(long)]
        ollama_model: Option<String>,
        /// Ollama endpoint URL
        #[arg(long)]
        ollama_endpoint: Option<String>,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        match cli.command {
            Commands::Gen { modality } => match modality {
                GenModality::Text { prompt, verbose, json, influence, cid, log_file, stream, ollama_model, ollama_endpoint } => {
                    gen_text(prompt, verbose, json, influence, cid, log_file, stream, ollama_model, ollama_endpoint).await
                }
            },
        }
    });
    
    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

#[derive(serde::Serialize)]
struct LogEnvelope {
    correlation_id: String,
    modality: String,
    content: String,
    constraints: Vec<ConstraintResult>,
    events: Vec<ResonanceEvent>,
    timestamp_unix_s: u64,
    influence: InfluenceSnapshot,
}

async fn gen_text(prompt: String, verbose: bool, json: bool, influence_flag: bool, cid: Option<String>, log_file: Option<PathBuf>, stream: bool, ollama_model: Option<String>, ollama_endpoint: Option<String>) -> Result<(), String> {
    // If Ollama parameters are provided, use Ollama for generation
    if let (Some(model), Some(endpoint)) = (ollama_model, ollama_endpoint) {
        return gen_text_ollama(&prompt, &model, &endpoint, stream, verbose).await;
    }

    // Fallback to BasicText if no Ollama parameters
    let mut orch = Orchestrator::new();
    orch.register_modality(BasicText);
    orch.set_meta_engine(MetaEngineImpl::new_default());

    info!("modalities = {:?}", orch.modalities());
    let req = GenerationRequest { prompt };
    if let Some(res) = orch.generate("text", req) {
        if stream {
            // Stream output line by line
            for line in res.content.lines() {
                println!("{}", line);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
        } else {
            println!("{}", res.content);
        }
        let (log, mut events) = orch.evaluate_with_events("text", &res.content);
        // Correlation ID event injection
        let cid_val = cid.unwrap_or_else(|| Uuid::new_v4().to_string());
        let corr_event = ResonanceEvent {
            operator: OperatorClass::InteractionNotice,
            message: format!("correlation_id: {}", cid_val),
            section_ref: None,
            symbol: None,
        };
        events.insert(0, corr_event);

        // Influence snapshot (logging-only)
        let (influence, _infl_notice) = compute_influence(&events);

        // Optional file logging of JSON envelope
        if let Some(path) = log_file {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let env = LogEnvelope {
                correlation_id: cid_val.clone(),
                modality: "text".into(),
                content: res.content.clone(),
                constraints: log.clone(),
                events: events.clone(),
                timestamp_unix_s: ts,
                influence: influence.clone(),
            };
            if let Ok(s) = serde_json::to_string_pretty(&env) {
                let _ = fs::write(path, s);
            }
        }
        if json {
            if !events.is_empty() {
                let s = serde_json::to_string_pretty(&events).unwrap_or_else(|_| "[]".into());
                println!("--- resonance(json) ---\n{}", s);
            }
        } else if verbose {
            if !events.is_empty() {
                println!("--- resonance ---");
                for e in events {
                    let sym = e.symbol.as_deref().unwrap_or("?");
                    let sec = e.section_ref.as_deref().unwrap_or("-");
                    println!("{} [{}] {}: {}", sym, sec, format!("{:?}", e.operator), e.message);
                }
            }
            if influence_flag {
                println!("--- influence ---");
                println!("resonance_index = {:.2}", influence.resonance_index);
                if !influence.operator_influence.is_empty() {
                    println!("top = {}", influence.operator_influence.iter().take(3).map(|ow| format!("{:?}:{:.2}", ow.operator, ow.weight)).collect::<Vec<_>>().join(", "));
                }
                println!("cooperation = {}  conflict = {}", influence.cooperation_count, influence.conflict_count);
            }
        }
        if !log.is_empty() {
            println!("--- constraints ---");
            for r in log {
                println!("{} [{}]: {}", r.name, r.severity, r.message.unwrap_or_else(|| "ok".into()));
            }
        }
        Ok(())
    } else {
        warn!("generation failed or blocked by constraints");
        Err("generation failed or blocked by constraints".to_string())
    }
}

async fn gen_text_ollama(prompt: &str, model: &str, endpoint: &str, stream: bool, _verbose: bool) -> Result<(), String> {
    let client = Client::new();
    let url = format!("{}/api/generate", endpoint);
    
    let request_body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": stream
    });

    info!("Sending request to Ollama: {} model: {}", url, model);
    
    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to send request to Ollama: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Ollama request failed with status {}: {}", status, body));
    }

    if stream {
        // Handle streaming response
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Error reading stream: {}", e))?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);
            
            // Process complete lines
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();
                
                if !line.is_empty() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(response_text) = json.get("response").and_then(|v| v.as_str()) {
                            print!("{}", response_text);
                            std::io::Write::flush(&mut std::io::stdout()).ok();
                        }
                    }
                }
            }
        }
        
        // Process any remaining buffer
        if !buffer.trim().is_empty() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&buffer) {
                if let Some(response_text) = json.get("response").and_then(|v| v.as_str()) {
                    print!("{}", response_text);
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                }
            }
        }
    } else {
        // Handle non-streaming response
        let body = response.text().await.map_err(|e| format!("Failed to read response: {}", e))?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(response_text) = json.get("response").and_then(|v| v.as_str()) {
                println!("{}", response_text);
            } else {
                return Err("Invalid response format from Ollama".to_string());
            }
        } else {
            return Err("Failed to parse Ollama response as JSON".to_string());
        }
    }

    Ok(())
}
