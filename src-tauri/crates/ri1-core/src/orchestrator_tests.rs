use crate::orchestrator::Orchestrator;
use crate::modality::{GenerationRequest, GenerationResponse, Modality};
use crate::constraints::{MetaEngine, ConstraintResult, ResonanceEvent, FieldContext, Consent, OperatorDef, ConditionalDef};
use std::sync::{Arc, Mutex};

// Mock modality that returns a specific response
struct MockModality {
    response: String,
}

impl Modality for MockModality {
    fn name(&self) -> &'static str { "text" }
    fn generate(&self, _req: GenerationRequest) -> GenerationResponse {
        GenerationResponse { content: self.response.clone() }
    }
}

// Mock meta engine that counts evaluations
struct CountingMetaEngine {
    eval_count: Arc<Mutex<usize>>,
    last_content: Arc<Mutex<Option<String>>>,
}

impl MetaEngine for CountingMetaEngine {
    fn evaluate_meta(&self, _modality: &str, content: &str, _ctx: &FieldContext) -> (Vec<ConstraintResult>, Vec<ResonanceEvent>) {
        let mut count = self.eval_count.lock().unwrap();
        *count += 1;
        let mut last = self.last_content.lock().unwrap();
        *last = Some(content.to_string());
        
        // Return empty results for testing
        (Vec::new(), Vec::new())
    }

    fn consent_check(&self, _ctx: &FieldContext) -> Consent {
        Consent { granted: true, subject: None, reason: None, section_ref: None }
    }

    fn operators(&self) -> &[OperatorDef] {
        &[]
    }

    fn conditionals(&self) -> &[ConditionalDef] {
        &[]
    }
}

#[test]
fn test_exactly_one_evaluation() {
    let eval_count = Arc::new(Mutex::new(0));
    let last_content = Arc::new(Mutex::new(None));
    
    let mut orch = Orchestrator::new();
    orch.register_modality(MockModality { response: "Φπε-731".to_string() });
    orch.set_meta_engine(CountingMetaEngine {
        eval_count: eval_count.clone(),
        last_content: last_content.clone(),
    });
    
    let req = GenerationRequest { prompt: "test prompt".to_string() };
    let result = orch.generate_with_evaluation("text", req);
    
    assert!(result.is_some());
    let (response, _results, _events) = result.unwrap();
    assert_eq!(response.content, "Φπε-731");
    
    // Verify exactly one evaluation occurred
    let count = *eval_count.lock().unwrap();
    assert_eq!(count, 1, "Expected exactly 1 evaluation, got {}", count);
    
    // Verify the evaluation received the correct content
    let content = last_content.lock().unwrap();
    assert_eq!(content.as_ref().unwrap(), "Φπε-731");
}

#[test]
fn test_generate_still_works() {
    let eval_count = Arc::new(Mutex::new(0));
    let last_content = Arc::new(Mutex::new(None));
    
    let mut orch = Orchestrator::new();
    orch.register_modality(MockModality { response: "test".to_string() });
    orch.set_meta_engine(CountingMetaEngine {
        eval_count: eval_count.clone(),
        last_content: last_content.clone(),
    });
    
    let req = GenerationRequest { prompt: "test".to_string() };
    let result = orch.generate("text", req);
    
    assert!(result.is_some());
    assert_eq!(result.unwrap().content, "test");
    
    // generate() should also result in exactly one evaluation
    let count = *eval_count.lock().unwrap();
    assert_eq!(count, 1, "Expected exactly 1 evaluation, got {}", count);
}
