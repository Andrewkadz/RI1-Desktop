mod basin;
pub mod subprocess;
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::Emitter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use subprocess::{ProcessRegistry, SubprocessConfig, SubprocessLifecycle, SubprocessResult, StreamCallback, cancel_subprocess, create_topic};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct CrmSnapshot {
    pub nodes: Vec<String>,
    pub matrix: Vec<Vec<f32>>,
}

fn normalize_protocol_name(raw: &str) -> String {
    let mut name = raw.trim().trim_matches(|c: char| c == '"' || c == '\'');
    if name.is_empty() {
        name = "Research Protocol";
    }

    // Ensure there is at least one separator " – " so the title reads like a protocol label
    let mut parts: Vec<&str> = name.split('–').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        // Try to split on "-" if no en dash is present
        parts = name
            .split('-')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
    }

    if parts.is_empty() {
        parts.push("Research");
        parts.push("Unnamed Protocol");
    } else if parts.len() == 1 {
        parts.insert(0, "Research");
    }

    let mut base = parts.join(" – ");

    // Ensure there is a version suffix like v0.1
    if !base.trim().ends_with(|c: char| c == '1' || c == '2' || c == '3' || c == '4' || c == '5' || c == '6' || c == '7' || c == '8' || c == '9' || c == '0')
        || !base.contains('v')
    {
        if !base.to_lowercase().contains(" v") {
            base.push_str(" – v0.1");
        }
    }

    // Hard cap to keep names readable
    if base.len() > 160 {
        base.truncate(157);
        base.push_str("…");
    }
    base
}

// === Recursive Protocol Object storage (Phase 1) ===

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct RecursiveProtocolObject {
    pub id: String,
    pub name: String,
    pub description: String,
    pub origin: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: String,
    pub schema_version: String,
    #[serde(default)]
    pub raw_text: Option<String>,
}

#[derive(Clone)]
struct ProtocolStore(Arc<Mutex<HashMap<String, RecursiveProtocolObject>>>);

impl ProtocolStore {
    fn new() -> Self {
        ProtocolStore(Arc::new(Mutex::new(HashMap::new())))
    }

    fn save(&self, proto: RecursiveProtocolObject) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(proto.id.clone(), proto);
        }
    }

    fn list(&self) -> Vec<RecursiveProtocolObject> {
        self.0
            .lock()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    fn get(&self, id: &str) -> Option<RecursiveProtocolObject> {
        self.0
            .lock()
            .ok()
            .and_then(|m| m.get(id).cloned())
    }
}

// === PDF Node library (Phase 1) ===

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PdfNode {
    pub id: String,
    pub name: String,
    pub path: String,
    pub hash: String,
    pub page_count: u32,
    pub chunk_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub block_ids: Vec<String>,
}

#[derive(Clone)]
struct PdfLibraryStore(Arc<Mutex<HashMap<String, PdfNode>>>);

impl PdfLibraryStore {
    fn new() -> Self {
        let map = Self::load_from_disk();
        PdfLibraryStore(Arc::new(Mutex::new(map)))
    }

    fn library_file_path() -> PathBuf {
        PathBuf::from("pdf_nodes.json")
    }

    fn load_from_disk() -> HashMap<String, PdfNode> {
        let path = Self::library_file_path();
        if let Ok(data) = fs::read_to_string(path) {
            if let Ok(list) = serde_json::from_str::<Vec<PdfNode>>(&data) {
                let mut map = HashMap::new();
                for n in list {
                    map.insert(n.id.clone(), n);
                }
                return map;
            }
        }
        HashMap::new()
    }

    fn persist(&self) {
        if let Ok(m) = self.0.lock() {
            let list: Vec<PdfNode> = m.values().cloned().collect();
            if let Ok(data) = serde_json::to_string_pretty(&list) {
                let path = Self::library_file_path();
                let _ = fs::write(path, data);
            }
        }
    }

    fn register(&self, node: PdfNode) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(node.id.clone(), node);
        }
        self.persist();
    }

    fn list(&self) -> Vec<PdfNode> {
        self.0
            .lock()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    fn get(&self, id: &str) -> Option<PdfNode> {
        self.0
            .lock()
            .ok()
            .and_then(|m| m.get(id).cloned())
    }

    fn delete(&self, id: &str) {
        if let Ok(mut m) = self.0.lock() {
            let _ = m.remove(id);
        }
        self.persist();
    }

    fn find_by_hash_or_path(&self, hash: &str, path: &str) -> Option<PdfNode> {
        self.0
            .lock()
            .ok()
            .and_then(|m| {
                m.values().find(|n| n.hash == hash || n.path == path).cloned()
            })
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// Basic RPO commands (Phase 1 wiring)

#[tauri::command]
fn save_protocol(store: tauri::State<'_, ProtocolStore>, proto: RecursiveProtocolObject) -> Result<(), String> {
    store.save(proto);
    Ok(())
}

#[tauri::command]
fn list_protocols(store: tauri::State<'_, ProtocolStore>) -> Result<Vec<RecursiveProtocolObject>, String> {
    Ok(store.list())
}

#[tauri::command]
fn get_protocol(store: tauri::State<'_, ProtocolStore>, id: String) -> Result<Option<RecursiveProtocolObject>, String> {
    Ok(store.get(&id))
}

// Basic PdfNode library commands (Phase 1 wiring)
#[tauri::command]
fn register_pdf_node(store: tauri::State<'_, PdfLibraryStore>, node: PdfNode) -> Result<(), String> {
    store.register(node);
    Ok(())
}

#[tauri::command]
fn list_pdf_nodes(store: tauri::State<'_, PdfLibraryStore>) -> Result<Vec<PdfNode>, String> {
    Ok(store.list())
}

#[tauri::command]
fn get_pdf_node(store: tauri::State<'_, PdfLibraryStore>, id: String) -> Result<Option<PdfNode>, String> {
    Ok(store.get(&id))
}

#[tauri::command]
fn delete_pdf_node(store: tauri::State<'_, PdfLibraryStore>, id: String) -> Result<(), String> {
    store.delete(&id);
    Ok(())
}

#[tauri::command]
fn update_pdf_metadata(
    store: tauri::State<'_, PdfLibraryStore>,
    id: String,
    page_count: Option<u32>,
    chunk_count: Option<u32>,
    block_ids: Option<Vec<String>>,
) -> Result<PdfNode, String> {
    let mut node = store.get(&id).ok_or_else(|| format!("PDF node {} not found", id))?;
    
    if let Some(pc) = page_count {
        node.page_count = pc;
    }
    if let Some(cc) = chunk_count {
        node.chunk_count = cc;
    }
    if let Some(ids) = block_ids {
        node.block_ids = ids;
    }
    node.updated_at = now_millis();
    
    store.register(node.clone());
    Ok(node)
}

#[tauri::command]
async fn read_pdf_file(path: String) -> Result<Vec<u8>, String> {
    fs::read(&path).map_err(|e| format!("Failed to read PDF file: {}", e))
}

#[tauri::command]
async fn index_pdf_stream(
    app: tauri::AppHandle,
    thread_id: String,
    file_bytes: Vec<u8>,
) -> Result<Vec<String>, String> {
    use pdf_extract::extract_text_from_mem;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    
    // Extract all text from PDF
    let text = extract_text_from_mem(&file_bytes)
        .map_err(|e| format!("Failed to extract PDF text: {}", e))?;
    
    if text.trim().is_empty() {
        return Err("PDF contains no extractable text".to_string());
    }
    
    // Chunk by ~10 pages worth of text (30,000 chars ~= 10 pages)
    let chars_per_chunk = 30000;
    let mut chunk_ids = Vec::new();
    let mut chunk_number = 0;
    let mut start = 0;
    
    while start < text.len() {
        let mut end = (start + chars_per_chunk).min(text.len());
        
        // Intelligent boundary detection within last 2000 chars
        if end < text.len() && end > start + 2000 {
            let search_start = end - 2000;
            let search_window = &text[search_start..end];
            
            // Try to detect section headers (lines with CAPITALS or numbers)
            let header_patterns = [
                "\n\n[A-Z][A-Z ]+\n", // ALL CAPS headers
                "\n\n\\d+\\.\\s+[A-Z]", // Numbered sections like "1. Introduction"
                "\n\n[A-Z][a-z]+(?:\\s+[A-Z][a-z]+)*\\n", // Title Case Headers
            ];
            
            let mut found_header = false;
            for pattern in &header_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if let Some(mat) = re.find(search_window) {
                        end = search_start + mat.start() + 2;
                        found_header = true;
                        break;
                    }
                }
            }
            
            // If no header found, try paragraph boundary
            if !found_header {
                if let Some(pos) = search_window.rfind("\n\n") {
                    end = search_start + pos + 2;
                }
                // Then sentence boundary
                else if let Some(pos) = search_window.rfind(". ") {
                    end = search_start + pos + 2;
                }
            }
        }
        
        let chunk_text = &text[start..end];
        
        // Generate hash for this chunk
        let mut hasher = DefaultHasher::new();
        hasher.write(chunk_text.as_bytes());
        let chunk_hash = format!("{:x}", hasher.finish());
        
        let chunk_id = format!("chunk-{}-{:03}-{}", thread_id, chunk_number, &chunk_hash[..8]);
        chunk_ids.push(chunk_id.clone());
        
        // Emit chunk event
        let _ = app.emit("pdf_chunk", serde_json::json!({
            "threadId": thread_id,
            "chunkId": chunk_id,
            "text": chunk_text.trim(),
        }));
        
        chunk_number += 1;
        start = end; // No overlap
    }
    
    Ok(chunk_ids)
}

fn hash_file_contents(path: &Path) -> Result<String, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::io::Read;

    let mut file = fs::File::open(path).map_err(|e| format!("failed to open pdf: {}", e))?;
    let mut hasher = DefaultHasher::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("failed to read pdf: {}", e))?;
        if n == 0 { break; }
        buf[..n].hash(&mut hasher);
    }
    Ok(format!("{:x}", hasher.finish()))
}

fn import_single_pdf(store: &PdfLibraryStore, path_str: String) -> Result<PdfNode, String> {
    let path = Path::new(&path_str);
    if !path.exists() {
        return Err(format!("path does not exist: {}", path_str));
    }
    if !path.is_file() {
        return Err(format!("not a file: {}", path_str));
    }
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false);
    if !ext_ok {
        return Err("file is not a PDF".to_string());
    }

    let hash = hash_file_contents(path)?;

    if let Some(existing) = store.find_by_hash_or_path(&hash, &path_str) {
        return Ok(existing);
    }

    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled PDF")
        .to_string();
    let now = now_millis();
    let id = format!("pdf-{}", now);

    let node = PdfNode {
        id,
        name,
        path: path_str,
        hash,
        page_count: 0,
        chunk_count: 0,
        created_at: now,
        updated_at: now,
        block_ids: Vec::new(),
    };

    store.register(node.clone());
    Ok(node)
}

fn collect_pdf_paths(root: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(meta) = root.metadata() {
        if meta.is_file() {
            let is_pdf = root
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("pdf"))
                .unwrap_or(false);
            if is_pdf {
                out.push(root.to_path_buf());
            }
            return;
        }
    }

    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_pdf_paths(&path, out);
            } else if path.is_file() {
                let is_pdf = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("pdf"))
                    .unwrap_or(false);
                if is_pdf {
                    out.push(path);
                }
            }
        }
    }
}

#[tauri::command]
async fn import_pdf_into_library(store: tauri::State<'_, PdfLibraryStore>, path: String) -> Result<PdfNode, String> {
    import_single_pdf(&store, path)
}

#[tauri::command]
async fn import_pdf_folder_into_library(store: tauri::State<'_, PdfLibraryStore>, root: String) -> Result<Vec<PdfNode>, String> {
    let root_path = Path::new(&root);
    if !root_path.exists() {
        return Err(format!("root does not exist: {}", root));
    }
    let mut paths = Vec::new();
    collect_pdf_paths(root_path, &mut paths);
    eprintln!("DEBUG: Found {} PDF paths", paths.len());
    for (i, p) in paths.iter().enumerate().take(5) {
        eprintln!("DEBUG: Path {}: {:?}", i, p);
    }
    let mut imported = Vec::new();
    for p in paths {
        if let Some(s) = p.to_str() {
            match import_single_pdf(&store, s.to_string()) {
                Ok(node) => {
                    eprintln!("DEBUG: Imported: {}", node.name);
                    imported.push(node);
                }
                Err(e) => {
                    eprintln!("DEBUG: Failed to import {:?}: {}", p, e);
                }
            }
        }
    }
    eprintln!("DEBUG: Total imported: {}", imported.len());
    Ok(imported)
}

// Phase 2 stub: wrap raw protocol text into a minimal RecursiveProtocolObject
#[tauri::command]
fn parse_protocol_from_text(text: String) -> Result<RecursiveProtocolObject, String> {
    let now = now_millis();
    // Try to extract a `name:` field if present; otherwise fall back to first non-empty line
    let mut explicit_name: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("name") {
            if let Some(idx) = trimmed.find(':') {
                let value = trimmed[(idx + 1)..].trim();
                if !value.is_empty() {
                    explicit_name = Some(value.to_string());
                    break;
                }
            }
        }
    }

    let first_non_empty = text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");

    let raw_name = explicit_name.as_deref().unwrap_or(first_non_empty);
    let name = normalize_protocol_name(raw_name);
    let id = format!("rpo-{}", now);
    let proto = RecursiveProtocolObject {
        id,
        name,
        description: String::new(),
        origin: "unknown".to_string(),
        created_at: now,
        updated_at: now,
        status: "draft".to_string(),
        schema_version: "rpo.v1".to_string(),
        raw_text: Some(text),
    };
    Ok(proto)
}

#[derive(serde::Deserialize, serde::Serialize)]
struct OllamaShowResp {
    #[serde(default)]
    modelfile: String,
    #[serde(default)]
    parameters: serde_json::Value,
}

// Query Ollama `/api/show` to fetch Modelfile and parameters for a model
#[tauri::command]
async fn ollama_show(model: String, endpoint: Option<String>) -> Result<OllamaShowResp, String> {
    use std::process::{Command, Stdio};
    let ep = endpoint.unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let url = format!("{}/api/show", ep.trim_end_matches('/'));
    let payload = serde_json::json!({"name": model});
    let body_str = payload.to_string();
    let output = tauri::async_runtime::spawn_blocking(move || {
        Command::new("curl")
            .arg("-sS")
            .arg("-X").arg("POST")
            .arg(url)
            .arg("-H").arg("Content-Type: application/json")
            .arg("-d").arg(body_str)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("failed to run curl: {}", e))
    }).await.map_err(|e| format!("task join error: {}", e))??;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let body = String::from_utf8_lossy(&output.stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("invalid json: {}", e))?;
    Ok(OllamaShowResp {
        modelfile: v.get("modelfile").and_then(|m| m.as_str()).unwrap_or("").to_string(),
        parameters: v.get("parameters").cloned().unwrap_or(serde_json::Value::Null),
    })
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// Ollama chat over HTTP (/api/chat) via curl.
// Using the HTTP chat API instead of driving `ollama run` means the model's
// chat template and stop tokens are applied properly, messages carry real
// roles, and no terminal control codes leak into the reply.
fn ollama_chat_body(prompt: &str, model: &str, stream: bool) -> String {
    serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": stream
    })
    .to_string()
}

fn spawn_ollama_chat(body: &str) -> Result<std::process::Child, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("curl")
        .args([
            "-sS", "-N", "-X", "POST", "http://127.0.0.1:11434/api/chat",
            "-H", "Content-Type: application/json", "--data-binary", "@-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run curl: {}", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(body.as_bytes())
            .map_err(|e| format!("failed to send request to ollama: {}", e))?;
    } // stdin dropped here -> request body complete
    Ok(child)
}

// ---------------------------------------------------------------- Workshop
// Generic access to a small allowlist of Ollama HTTP endpoints, so the
// Workshop tab can tune, benchmark and inspect models. Returns raw JSON text.
#[tauri::command]
async fn ollama_api(path: String, body: Option<String>, endpoint: Option<String>) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let allowed_post = ["chat", "show"];
    let allowed_get = ["tags", "ps", "version"];
    let is_post = allowed_post.contains(&path.as_str());
    if !is_post && !allowed_get.contains(&path.as_str()) {
        return Err(format!("endpoint not allowed: {}", path));
    }
    let ep = endpoint.unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let url = format!("{}/api/{}", ep.trim_end_matches('/'), path);
    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new("curl");
        cmd.arg("-sS").arg("--max-time").arg("900");
        if is_post {
            cmd.args(["-X", "POST", "-H", "Content-Type: application/json", "--data-binary", "@-"]);
        }
        cmd.arg(&url).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("failed to run curl: {}", e))?;
        if let Some(mut stdin) = child.stdin.take() {
            if is_post {
                stdin
                    .write_all(body.unwrap_or_else(|| "{}".to_string()).as_bytes())
                    .map_err(|e| format!("failed to send request: {}", e))?;
            }
        }
        let out = child.wait_with_output().map_err(|e| format!("curl error: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "could not reach Ollama at {} (is it running?): {}",
                url,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?
}

fn workshop_path(file: &str) -> Result<std::path::PathBuf, String> {
    let allowed = ["notebook.jsonl", "bench.jsonl"];
    if !allowed.contains(&file) {
        return Err(format!("file not allowed: {}", file));
    }
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    let dir = std::path::Path::new(&home).join("RI1-Workshop");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    Ok(dir.join(file))
}

#[tauri::command]
fn workshop_dir() -> Result<String, String> {
    let p = workshop_path("notebook.jsonl")?;
    Ok(p.parent().map(|d| d.display().to_string()).unwrap_or_default())
}

#[tauri::command]
fn workshop_read(file: String) -> Result<String, String> {
    let p = workshop_path(&file)?;
    match std::fs::read_to_string(&p) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("cannot read {}: {}", p.display(), e)),
    }
}

#[tauri::command]
fn workshop_append(file: String, line: String) -> Result<(), String> {
    use std::io::Write;
    let p = workshop_path(&file)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(|e| format!("cannot open {}: {}", p.display(), e))?;
    writeln!(f, "{}", line.replace('\n', " ")).map_err(|e| format!("cannot write: {}", e))
}

#[tauri::command]
fn workshop_write(file: String, content: String) -> Result<(), String> {
    let p = workshop_path(&file)?;
    std::fs::write(&p, content).map_err(|e| format!("cannot write {}: {}", p.display(), e))
}

// RI1 Basin retrieval (read-only). See src/basin.rs.
#[derive(serde::Serialize)]
struct BasinResult {
    hits: Vec<basin::BasinHit>,
    dir: String,
    curated: usize,
    raw: usize,
}

#[tauri::command]
async fn basin_search(query: String, k: Option<usize>, basin_path: Option<String>) -> Result<BasinResult, String> {
    let dir = basin_path
        .filter(|p| !p.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(basin::default_dir);
    let k = k.unwrap_or(6).clamp(1, 12);
    tauri::async_runtime::spawn_blocking(move || {
        let (hits, curated, raw) = basin::cached_search(&dir, &query, k, 6000)?;
        Ok(BasinResult { hits, dir: dir.display().to_string(), curated, raw })
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?
}

// Non-streaming chat (used as the fallback path).
#[tauri::command]
async fn gen_ollama(prompt: String, model: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let child = spawn_ollama_chat(&ollama_chat_body(&prompt, &model, false))?;
        let out = child
            .wait_with_output()
            .map_err(|e| format!("failed to read ollama output: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "ollama request failed (is Ollama running?): {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| format!("invalid response from ollama: {}", e))?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(format!("ollama: {}", err));
        }
        Ok(v["message"]["content"].as_str().unwrap_or("").to_string())
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?
}

// Fetch CRM switchboard by calling `ri1-cli crm --dump` and parsing JSON.
#[tauri::command]
async fn get_crm_switchboard(ri1_cli_path: Option<String>) -> Result<CrmSnapshot, String> {
    use std::process::{Command, Stdio};

    // Simple finder mirroring existing ri1-cli lookup logic
    fn find_ri1_cli(override_path: Option<String>) -> Option<String> {
        use std::env;
        if let Some(p) = override_path {
            if Command::new(&p).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
                return Some(p);
            }
        }
        if let Ok(p) = env::var("RI1_CLI_PATH") {
            if !p.is_empty() && Command::new(&p).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
                return Some(p);
            }
        }
        if Command::new("ri1-cli").arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
            return Some("ri1-cli".into());
        }
        if let Ok(home) = env::var("HOME") {
            for c in [
                format!("{}/.cargo/bin/ri1-cli", home),
                "/opt/homebrew/bin/ri1-cli".into(),
                "/usr/local/bin/ri1-cli".into(),
                format!("{}/Desktop/AI/ri1-hybrid-engine/target/debug/ri1-cli", home),
                format!("{}/Desktop/AI/ri1-hybrid-engine/target/release/ri1-cli", home),
            ] {
                if Command::new(&c).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
                    return Some(c);
                }
            }
        }
        None
    }

    let output = tauri::async_runtime::spawn_blocking(move || {
        let ri1_bin = find_ri1_cli(ri1_cli_path)
            .ok_or_else(|| "could not find 'ri1-cli'".to_string())?;
        let out = Command::new(ri1_bin)
            .arg("crm")
            .arg("--dump")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("failed to spawn ri1-cli: {}", e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(format!("ri1-cli crm failed: {}", stderr));
        }
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let snap: CrmSnapshot = serde_json::from_str(&stdout)
            .map_err(|e| format!("failed to parse CRM json: {}", e))?;
        Ok(snap)
    }).await.map_err(|e| format!("task join error: {}", e))??;

    Ok(output)
}

#[derive(serde::Serialize, Clone)]
struct StreamChunk<'a> {
    chunk: &'a str,
    done: bool,
}

// Streaming: emit events to the frontend with a caller-provided stream_id.
// Frontend should listen on `ollama://{stream_id}`.
// Reads Ollama's newline-delimited JSON stream and forwards only the reply text.
#[tauri::command]
async fn gen_ollama_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessRegistry>,
    prompt: String,
    model: String,
    stream_id: String,
) -> Result<(), String> {
    let stream_topic = format!("ollama://{}", stream_id);
    let registry = state.inner().clone();

    // Reserve request in registry at command entry (before async dispatch)
    let cancel_token = registry.insert_pending(stream_topic.clone());

    tauri::async_runtime::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};

        // Check if cancelled before spawning
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            registry.remove(&stream_topic);
            return Ok(());
        }

        let mut child = match spawn_ollama_chat(&ollama_chat_body(&prompt, &model, true)) {
            Ok(c) => c,
            Err(e) => {
                // Cleanup on spawn failure
                registry.remove(&stream_topic);
                return Err(e);
            }
        };

        // Register child PID immediately after spawning (before cancellation check)
        registry.insert(stream_topic.clone(), child.id());

        // Check cancellation after spawn and PID registration
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            registry.remove(&stream_topic);
            return Ok(());
        }

        let mut err_msg: Option<String> = None;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines() {
                // Check cancellation during streaming
                if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let line = match line { Ok(l) => l, Err(_) => break };
                if line.trim().is_empty() { continue; }
                let v: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
                    err_msg = Some(e.to_string());
                    break;
                }
                if let Some(s) = v["message"]["content"].as_str() {
                    if !s.is_empty() {
                        let _ = app.emit(&stream_topic, StreamChunk { chunk: s, done: false });
                    }
                }
                if v["done"].as_bool() == Some(true) { break; }
            }
        }

        let status = child.wait().map_err(|e| {
            // Cleanup registry on wait failure
            registry.remove(&stream_topic);
            format!("wait error: {}", e)
        })?;
        registry.remove(&stream_topic);
        // Only emit done on success - failed exits should reach error handling
        if let Some(e) = err_msg {
            return Err(format!("ollama: {}", e));
        }
        if status.success() {
            let _ = app.emit(&stream_topic, StreamChunk { chunk: "", done: true });
            Ok(())
        } else {
            Err(format!("ollama request failed with status: {}", status))
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))??;

    Ok(())
}

// Attempt to cancel a running stream by killing the tracked PID.
#[tauri::command]
async fn cancel_stream(state: tauri::State<'_, ProcessRegistry>, stream_id: String, backend: Option<String>) -> Result<(), String> {
    // Compose key with scheme used when registering
    let backend_str = backend.as_deref().unwrap_or("ri1");
    let key = create_topic(backend_str, &stream_id);
    cancel_subprocess(state.inner(), &key)
}

#[derive(serde::Serialize)]
struct OllamaHealth {
    running: bool,
    endpoint: String,
    models: Vec<String>,
}

// Query Ollama `/api/tags` via `curl` and return model names.
#[tauri::command]
async fn get_ollama_tags(endpoint: Option<String>) -> Result<Vec<String>, String> {
    use std::process::{Command, Stdio};
    let ep = endpoint.unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let url = format!("{}/api/tags", ep.trim_end_matches('/'));
    let output = tauri::async_runtime::spawn_blocking(move || {
        Command::new("curl")
            .arg("-sS")
            .arg(url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("failed to run curl: {}", e))
    }).await.map_err(|e| format!("task join error: {}", e))??;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let body = String::from_utf8_lossy(&output.stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("invalid json: {}", e))?;
    let mut models: Vec<String> = vec![];
    if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
        for m in arr {
            if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                models.push(name.to_string());
            }
        }
    }
    Ok(models)
}

// Health check: is daemon reachable and which models are available
#[tauri::command]
async fn check_ollama(endpoint: Option<String>) -> Result<OllamaHealth, String> {
    let ep = endpoint.unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    match get_ollama_tags(Some(ep.clone())).await {
        Ok(models) => Ok(OllamaHealth { running: true, endpoint: ep, models }),
        Err(_) => Ok(OllamaHealth { running: false, endpoint: ep, models: vec![] }),
    }
}

// Stream `ri1-cli kosmos` stdout to the frontend. Minimal flags supported.
#[tauri::command]
async fn run_kosmos_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessRegistry>,
    topic: String,
    offline: bool,
    emit_raw: bool,
    ollama_model: Option<String>,
    prompt: String,
    ri1_cli_path: Option<String>,
    kosmos_path: Option<String>,
) -> Result<(), String> {
    let stream_topic = format!("kosmos://{}", topic);
    let registry = state.inner().clone();

    // Reserve request in registry at command entry (before async dispatch)
    let cancel_token = registry.insert_pending(stream_topic.clone());

    tauri::async_runtime::spawn_blocking(move || {
        use std::env;
        use std::process::{Command, Stdio};

        // Check if cancelled before proceeding with CLI discovery
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            registry.remove(&stream_topic);
            return Ok(());
        }

        fn find_ri1_cli(override_path: Option<String>) -> Option<String> {
            if let Some(p) = override_path {
                if Command::new(&p).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
                    return Some(p);
                }
            }
            // 1) Respect explicit env override
            if let Ok(p) = env::var("RI1_CLI_PATH") {
                if !p.is_empty() {
                    if Command::new(&p).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
                        return Some(p);
                    }
                }
            }
            // 2) PATH
            if Command::new("ri1-cli").arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
                return Some("ri1-cli".to_string());
            }
            // 3) Common locations
            let mut candidates: Vec<String> = vec![
                "/opt/homebrew/bin/ri1-cli".into(),
                "/usr/local/bin/ri1-cli".into(),
            ];
            if let Ok(home) = env::var("HOME") {
                candidates.push(format!("{}/.cargo/bin/ri1-cli", home));
                // Typical workspace debug/release builds
                candidates.push(format!("{}/Desktop/AI/ri1-hybrid-engine/target/debug/ri1-cli", home));
                candidates.push(format!("{}/Desktop/AI/ri1-hybrid-engine/target/release/ri1-cli", home));
            }
            for c in candidates {
                if Command::new(&c).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() {
                    return Some(c);
                }
            }
            None
        }

        fn find_kosmos_path(override_path: Option<String>) -> String {
            if let Some(p) = override_path { return p; }
            if let Ok(p) = env::var("KOSMOS_PATH") { if !p.is_empty() { return p; } }
            if let Ok(home) = env::var("HOME") {
                return format!("{}/Desktop/AI/Kosmos-ONE", home);
            }
            // Fallback: current dir 'Kosmos-ONE'
            "Kosmos-ONE".to_string()
        }

        // Build args
        let kosmos_path = find_kosmos_path(kosmos_path);
        let mut args: Vec<String> = vec![
            "kosmos".into(),
            "--question".into(), prompt,
            "--kosmos-path".into(), kosmos_path,
        ];
        if offline { args.push("--offline".into()); }
        if emit_raw { args.push("--emit-raw".into()); }
        if let Some(m) = ollama_model { args.push("--ollama-model".into()); args.push(m); }

        // Check cancellation again before CLI discovery
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            registry.remove(&stream_topic);
            return Ok(());
        }

        let ri1_bin = match find_ri1_cli(ri1_cli_path) {
            Some(bin) => bin,
            None => {
                // Clean up reservation on discovery failure
                registry.remove(&stream_topic);
                return Err("could not find 'ri1-cli' in PATH or common locations; set RI1_CLI_PATH to override".to_string());
            }
        };

        // Check cancellation again after CLI discovery
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            registry.remove(&stream_topic);
            return Ok(());
        }

        // Create Tauri callback for event emission
        let callback = Arc::new(TauriStreamCallback {
            app: app.clone(),
        });

        // Use shared subprocess lifecycle with existing cancellation token
        let config = SubprocessConfig {
            command: ri1_bin,
            args,
            topic: stream_topic.clone(),
            registry: registry.clone(),
            callback,
            stdin_null: true,
            stdout_piped: true,
            stderr_piped: true,
            simulate_wait_error: false,
            cancel_token: Some(cancel_token.clone()),
        };

        let lifecycle = SubprocessLifecycle::new(config);
        match lifecycle.execute()? {
            SubprocessResult::Success => Ok(()),
            SubprocessResult::Failure(err) => Err(err),
            SubprocessResult::Cancelled => Ok(()), // Cancellation is not an error
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))??;

    Ok(())
}

#[derive(serde::Serialize, Clone)]
struct Ri1Chunk<'a> {
    chunk: &'a str,
    done: bool,
}

// Tauri callback implementation for StreamCallback
struct TauriStreamCallback {
    app: tauri::AppHandle,
}

impl StreamCallback for TauriStreamCallback {
    fn emit_chunk(&self, topic: &str, chunk: &str, done: bool) {
        let _ = self.app.emit(topic, Ri1Chunk { chunk, done });
    }

    fn emit_error(&self, topic: &str, error: &str) {
        // For errors, emit a chunk with done=false and the error message prefixed with [err]
        // The frontend will handle this as an error, not a successful completion
        let error_chunk = format!("[err] {}", error);
        let _ = self.app.emit(topic, Ri1Chunk { chunk: &error_chunk, done: false });
    }
}

// Stream `ri1-cli gen text` output to the frontend.
#[tauri::command]
async fn gen_ri1_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessRegistry>,
    prompt: String,
    model: Option<String>,
    ri1_cli_path: Option<String>,
    ollama_endpoint: Option<String>,
    stream_id: String,
    verbose: Option<bool>,
) -> Result<(), String> {
    let topic = format!("ri1://{}", stream_id);
    let registry = state.inner().clone();

    // Reserve request in registry at command entry (before async dispatch)
    let cancel_token = registry.insert_pending(topic.clone());

    tauri::async_runtime::spawn_blocking(move || {
        use std::env;
        use std::process::{Command, Stdio};

        // Check if cancelled before proceeding with CLI discovery
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            registry.remove(&topic);
            return Ok(());
        }

        fn find_ri1_cli(override_path: Option<String>) -> Option<String> {
            if let Some(p) = override_path {
                if Command::new(&p).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() { return Some(p); }
            }
            if let Ok(p) = env::var("RI1_CLI_PATH") {
                if !p.is_empty() && Command::new(&p).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() { return Some(p); }
            }
            if Command::new("ri1-cli").arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() { return Some("ri1-cli".into()); }
            if let Ok(home) = env::var("HOME") {
                for c in [
                    format!("{}/.cargo/bin/ri1-cli", home),
                    "/opt/homebrew/bin/ri1-cli".into(),
                    "/usr/local/bin/ri1-cli".into(),
                    format!("{}/Desktop/AI/ri1-hybrid-engine/target/debug/ri1-cli", home),
                    format!("{}/Desktop/AI/ri1-hybrid-engine/target/release/ri1-cli", home),
                ] { if Command::new(&c).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().ok().is_some() { return Some(c); } }
            }
            None
        }

        // sanitize prompt: remove NUL and control characters that break argv
        fn sanitize(s: &str) -> String {
            s.chars()
                .map(|c| if c == '\u{0000}' { ' ' } else { c })
                .map(|c| if c.is_control() && c != '\n' && c != '\r' && c != '\t' { ' ' } else { c })
                .collect()
        }
        let safe_prompt = sanitize(&prompt);

        // Check cancellation again before CLI discovery
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            registry.remove(&topic);
            return Ok(());
        }

        let ri1_bin = match find_ri1_cli(ri1_cli_path) {
            Some(bin) => bin,
            None => {
                // Clean up reservation on discovery failure
                registry.remove(&topic);
                return Err("could not find 'ri1-cli'".to_string());
            }
        };

        // Check cancellation again after CLI discovery
        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
            registry.remove(&topic);
            return Ok(());
        }

        let mut args: Vec<String> = vec!["gen".into(), "text".into(), "--prompt".into(), safe_prompt, "--stream".into()];
        if let Some(m) = model {
            let name = m.trim().to_string();
            args.push("--ollama-model".into());
            args.push(name);
        }
        if let Some(ep) = ollama_endpoint {
            let e = ep.trim().to_string();
            if !e.is_empty() { args.push("--ollama-endpoint".into()); args.push(e); }
        }
        if verbose.unwrap_or(false) { args.push("--verbose".into()); }

        // Create Tauri callback for event emission
        let callback = Arc::new(TauriStreamCallback {
            app: app.clone(),
        });

        // Use shared subprocess lifecycle with existing cancellation token
        let config = SubprocessConfig {
            command: ri1_bin,
            args,
            topic: topic.clone(),
            registry: registry.clone(),
            callback,
            stdin_null: true,
            stdout_piped: true,
            stderr_piped: true,
            simulate_wait_error: false,
            cancel_token: Some(cancel_token.clone()),
        };

        let lifecycle = SubprocessLifecycle::new(config);
        match lifecycle.execute()? {
            SubprocessResult::Success => Ok(()),
            SubprocessResult::Failure(err) => Err(err),
            SubprocessResult::Cancelled => Ok(()), // Cancellation is not an error
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))??;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ProcessRegistry::new())
        .manage(ProtocolStore::new())
        .manage(PdfLibraryStore::new())
        .invoke_handler(tauri::generate_handler![
            greet,
            gen_ollama,
            gen_ollama_stream,
            run_kosmos_stream,
            gen_ri1_stream,
            get_crm_switchboard,
            get_ollama_tags,
            check_ollama,
            cancel_stream,
            ollama_show,
            save_protocol,
            list_protocols,
            get_protocol,
            parse_protocol_from_text,
            register_pdf_node,
            list_pdf_nodes,
            get_pdf_node,
            delete_pdf_node,
            update_pdf_metadata,
            read_pdf_file,
            index_pdf_stream,
            import_pdf_into_library,
            import_pdf_folder_into_library,
            ollama_api,
            workshop_dir,
            workshop_read,
            workshop_append,
            workshop_write,
            basin_search,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
