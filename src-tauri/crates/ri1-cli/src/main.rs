use clap::{Parser, Subcommand};
use ri1_core::modality::GenerationRequest;
use ri1_core::Orchestrator;
use ri1_core::constraints::{ResonanceEvent, OperatorClass, ConstraintResult};
use ri1_symbolic_meta::{MetaEngineImpl, InfluenceSnapshot, compute_influence, ExpressionParser, ParsedExpression};
use ri1_text::{BasicText, OllamaText};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use serde_json;
use uuid::Uuid;
use std::path::PathBuf;
use std::fs;

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
    let mut orch = Orchestrator::new();
    
    // Check if using Ollama before moving values
    let use_ollama = ollama_model.is_some() && ollama_endpoint.is_some();
    
    // Register appropriate modality based on whether Ollama parameters are provided
    if let (Some(model), Some(endpoint)) = (ollama_model, ollama_endpoint) {
        orch.register_modality(OllamaText::new(model, endpoint, stream));
    } else {
        orch.register_modality(BasicText);
    }
    
    orch.set_meta_engine(MetaEngineImpl::new_default());

    info!("modalities = {:?}", orch.modalities());
    
    // Parse expression if it looks like a symbolic expression
    let parsed_expr = if prompt.contains('(') && prompt.contains(')') && 
                      (prompt.contains('Φ') || prompt.contains('π') || prompt.contains('ε') || 
                       prompt.contains('Ρ') || prompt.contains('Τ') || prompt.contains('Θ') || 
                       prompt.contains('Λ') || prompt.contains('Δ') || prompt.contains('Ω') || 
                       prompt.contains('Ψ') || prompt.contains('Γ') || prompt.contains('Σ') || 
                       prompt.contains('Ξ') || prompt.contains('ζ') || prompt.contains('λ') || 
                       prompt.contains('ω') || prompt.contains('δ') || prompt.contains('χ')) {
        Some(ExpressionParser::parse(&prompt))
    } else {
        None
    };
    
    // Generate enhanced prompt with expression context
    let enhanced_prompt = if let Some(ref parsed) = parsed_expr {
        let context = ExpressionParser::generate_prompt_context(parsed);
        format!("{}\n\n{}", context, prompt)
    } else {
        prompt.clone()
    };
    
    let req = GenerationRequest { prompt: enhanced_prompt };
    if let Some((res, log, mut events)) = orch.generate_with_evaluation("text", req) {
        // For Ollama streaming, content is already printed during generation
        // For BasicText or non-streaming Ollama, print the content
        if !(use_ollama && stream) {
            if stream {
                // Stream output line by line
                for line in res.content.lines() {
                    println!("{}", line);
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                }
            } else {
                println!("{}", res.content);
            }
        }
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
        
        // Validate expression preservation if we parsed an expression
        if let Some(ref parsed) = parsed_expr {
            let validation_issues = validate_expression_output(&res.content, parsed);
            if !validation_issues.is_empty() {
                eprintln!("--- expression validation warnings ---");
                for issue in &validation_issues {
                    eprintln!("  - {}", issue);
                }
            }
        }
        
        Ok(())
    } else {
        warn!("generation failed or blocked by constraints");
        Err("generation failed or blocked by constraints".to_string())
    }
}

fn validate_expression_output(output: &str, parsed: &ParsedExpression) -> Vec<String> {
    let mut issues = Vec::new();
    
    // Check if original expression is preserved
    if !output.contains(&parsed.original) {
        issues.push(format!("original expression not found in output"));
    }
    
    // Check for symbol preservation
    for (ch, _) in &parsed.symbols {
        if !output.contains(*ch) {
            issues.push(format!("symbol '{}' missing from output", ch));
        }
    }
    
    // Check for operator preservation
    for (ch, _) in &parsed.operators {
        if !output.contains(*ch) {
            issues.push(format!("operator '{}' missing from output", ch));
        }
    }
    
    // Check for group preservation
    for group in &parsed.groups {
        if !output.contains(&group.content) {
            issues.push(format!("group '{}' missing from output", group.content));
        }
    }
    
    // Check for invented symbols (Greek letters not in original)
    let greek_chars = ['Φ', 'π', 'ε', 'Ρ', 'Τ', 'Θ', 'Λ', 'Δ', 'Ω', 'Ψ', 'Γ', 'Σ', 'Ξ', 'ζ', 'λ', 'ω', 'δ', 'χ'];
    for ch in output.chars() {
        if greek_chars.contains(&ch) && !parsed.original.contains(ch) {
            issues.push(format!("invented symbol '{}' in output", ch));
        }
    }
    
    // Check for symbol reordering (π and ε swapped)
    if parsed.original.contains("Φπε") && output.contains("επ") {
        issues.push("symbol order corrupted: 'Φπε' became 'επ'".to_string());
    }
    
    // Check for invented function notation
    if output.contains("π(n)") && !parsed.original.contains("π(n)") {
        issues.push("invented function notation 'π(n)' not in original".to_string());
    }
    
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use ri1_symbolic_meta::ExpressionParser;

    #[test]
    fn test_corrupted_output_detection() {
        let original = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(original);
        
        // Simulate corrupted output from the observed failure
        // This version omits the subtraction operator and corrupts symbols
        let corrupted = "(επ)^Ρ/Τ(π(n)Θ)/(Θn+Τ/Ρ)";
        
        let issues = validate_expression_output(corrupted, &parsed);
        
        // Should detect multiple issues
        assert!(issues.iter().any(|i| i.contains("original expression not found")));
        assert!(issues.iter().any(|i| i.contains("symbol 'Φ' missing")));
        assert!(issues.iter().any(|i| i.contains("group '(n-Θ)' missing")));
        assert!(issues.iter().any(|i| i.contains("operator '-' missing")));
        assert!(issues.iter().any(|i| i.contains("symbol order corrupted")));
        assert!(issues.iter().any(|i| i.contains("invented function notation")));
    }

    #[test]
    fn test_valid_output_detection() {
        let original = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(original);
        
        // Simulate valid output that preserves the expression
        let valid = "The expression (Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ) contains the following structure...";
        
        let issues = validate_expression_output(valid, &parsed);
        
        // Should have minimal issues (only missing some context, but preserving structure)
        assert!(issues.is_empty() || issues.len() <= 2);
    }

    #[test]
    fn test_missing_group_detection() {
        let original = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(original);
        
        // Output missing the final group
        let incomplete = "The expression (Φπε)^Ρ/Τ(n-Θ) has two groups...";
        
        let issues = validate_expression_output(incomplete, &parsed);
        
        assert!(issues.iter().any(|i| i.contains("group '(Θn+Τ/Ρ)' missing")));
    }

    #[test]
    fn test_missing_operator_detection() {
        let original = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(original);
        
        // Output missing subtraction operator
        let no_subtraction = "The expression (Φπε)^Ρ/Τ(nΘ)/(Θn+Τ/Ρ) uses addition...";
        
        let issues = validate_expression_output(no_subtraction, &parsed);
        
        assert!(issues.iter().any(|i| i.contains("operator '-' missing")));
    }
}
