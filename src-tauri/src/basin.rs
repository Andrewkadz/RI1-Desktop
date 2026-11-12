//! RI1 Basin retrieval for RI1 Desktop.
//!
//! Implements the retrieval step described in RI1-BASIN-DESIGN.md:
//! BM25 over the curated tier first (T1, segments.jsonl), raw chunks as
//! fallback (T2, chunks.jsonl), with tier labels and de-duplication.
//! The Basin is read-only: nothing here writes to it.
//!
//! Serving rules (from the Basin's own audit rules):
//! - chunks marked `suspect` are never served;
//! - chunks that are near-duplicates of another chunk are skipped;
//! - demoted segments are skipped, pinned segments get a small boost;
//! - a T2 chunk overlapping an already-selected T1 segment is skipped.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

#[derive(serde::Serialize, Clone, Debug)]
pub struct BasinHit {
    /// "T1" (curated segment) or "T2" (raw chunk)
    pub tier: String,
    pub id: String,
    pub doc_id: String,
    pub doc_path: String,
    pub page: i64,
    pub heading: String,
    pub text: String,
    pub score: f64,
}

#[derive(Clone, Debug)]
struct Item {
    tier: &'static str,
    id: String,
    doc_id: String,
    doc_path: String,
    page: i64,
    heading: String,
    start: i64,
    end: i64,
    text: String,
    pinned: bool,
    tf: HashMap<String, u32>,
    len: f64,
}

struct Tier {
    items: Vec<Item>,
    df: HashMap<String, u32>,
    avgdl: f64,
}

pub struct Index {
    t1: Tier,
    t2: Tier,
}

const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "of", "to", "in", "on", "for", "by", "with", "as", "at", "from", "is",
    "are", "was", "were", "be", "been", "it", "its", "this", "that", "these", "those", "what", "which",
    "who", "whom", "how", "why", "when", "where", "do", "does", "did", "can", "could", "should", "would",
    "will", "about", "into", "than", "then", "there", "their", "they", "them", "you", "your", "i", "me",
    "my", "we", "our", "us", "he", "she", "his", "her", "not", "no", "yes", "if", "so", "but", "also",
    "any", "all", "some", "such", "has", "have", "had", "use", "used", "using", "between", "each", "per",
    "say", "says", "tell", "explain", "describe", "give", "list", "according", "document", "documents",
];

/// Lowercased alphanumeric runs (Unicode-aware, so Greek words survive), plus
/// every non-ASCII letter on its own so a query for "Λ" can match "ΛΩΞΨ".
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                cur.push(lc);
            }
            if !ch.is_ascii() && ch.is_alphabetic() {
                out.push(ch.to_lowercase().collect::<String>());
            }
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|t| !(t.chars().count() == 1 && t.is_ascii()) && !STOP.contains(&t.as_str()));
    out.into_iter().map(stem).collect()
}

/// Very light English stemming so "thresholds"/"threshold" and
/// "compensates"/"compensation" meet. ASCII words only; Greek is left alone.
fn stem(t: String) -> String {
    if !t.is_ascii() || t.len() <= 4 {
        return t;
    }
    for suf in ["ations", "ation", "ions", "ion", "ings", "ing", "ies", "ied", "ers", "er", "ed", "es", "s"] {
        if t.len() > suf.len() + 3 && t.ends_with(suf) {
            let base = &t[..t.len() - suf.len()];
            return if suf == "ies" || suf == "ied" { format!("{}y", base) } else { base.to_string() };
        }
    }
    t
}

fn build_tier(items: Vec<Item>) -> Tier {
    let mut df: HashMap<String, u32> = HashMap::new();
    let mut total = 0.0;
    for it in &items {
        total += it.len;
        for t in it.tf.keys() {
            *df.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let avgdl = if items.is_empty() { 1.0 } else { total / items.len() as f64 };
    Tier { items, df, avgdl }
}

fn make_item(tier: &'static str, v: &serde_json::Value) -> Option<Item> {
    let s = |x: &serde_json::Value| x.as_str().unwrap_or("").to_string();
    let i = |x: &serde_json::Value| x.as_i64().unwrap_or(0);
    let (id, doc_id, doc_path, page, heading_v, start, end, pinned);
    if tier == "T1" {
        if v["selection"]["demoted"].as_bool() == Some(true) || v["extraction"]["suspect"].as_bool() == Some(true) {
            return None;
        }
        id = s(&v["segment_id"]);
        doc_id = s(&v["doc"]["doc_id"]);
        doc_path = s(&v["doc"]["doc_path"]);
        page = i(&v["locator"]["page"]);
        heading_v = v["locator"]["heading_path"].clone();
        start = i(&v["locator"]["char_start"]);
        end = i(&v["locator"]["char_end"]);
        pinned = v["selection"]["pinned"].as_bool() == Some(true);
    } else {
        if v["suspect"].as_bool() == Some(true) || !v["near_dup_of"].is_null() {
            return None;
        }
        id = s(&v["chunk_id"]);
        doc_id = s(&v["doc_id"]);
        doc_path = s(&v["doc_path"]);
        page = i(&v["page"]);
        heading_v = v["heading_path"].clone();
        start = i(&v["char_start"]);
        end = i(&v["char_end"]);
        pinned = false;
    }
    let text = s(&v["text"]);
    if text.trim().is_empty() {
        return None;
    }
    let heading = heading_v
        .as_array()
        .map(|a| a.iter().filter_map(|h| h.as_str()).collect::<Vec<_>>().join(" › "))
        .unwrap_or_default();
    // headings are part of what a passage is "about", so index them too
    let title = std::path::Path::new(&doc_path)
        .file_stem()
        .map(|f| f.to_string_lossy().replace(['_', '-'], " "))
        .unwrap_or_default();
    let mut toks = tokenize(&format!("{} {}", heading, text));
    let tt = tokenize(&format!("{} {}", title, doc_id.trim_start_matches("ri1:").replace('-', " ")));
    toks.extend(tt.iter().cloned());
    toks.extend(tt);
    let mut tf = HashMap::new();
    for t in &toks {
        *tf.entry(t.clone()).or_insert(0u32) += 1;
    }
    Some(Item { tier, id, doc_id, doc_path, page, heading, start, end, text, pinned, tf, len: toks.len().max(1) as f64 })
}

fn load_jsonl(path: &Path, tier: &'static str) -> Result<Vec<Item>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let mut out = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(it) = make_item(tier, &v) {
                out.push(it);
            }
        }
    }
    Ok(out)
}

pub fn load(dir: &Path) -> Result<Index, String> {
    let t1 = load_jsonl(&dir.join("segments.jsonl"), "T1")?;
    let t2 = load_jsonl(&dir.join("chunks.jsonl"), "T2")?;
    Ok(Index { t1: build_tier(t1), t2: build_tier(t2) })
}

/// BM25 scores for one tier; only items covering enough of the query survive.
fn score_tier(tier: &Tier, q: &[String]) -> Vec<(usize, f64)> {
    let (k1, b) = (1.2, 0.75);
    let n = tier.items.len() as f64;
    // a passage must contain all query terms for 1–2 term queries, 40% otherwise
    let need = if q.len() <= 2 { q.len() } else { ((q.len() as f64) * 0.4).ceil() as usize };
    let mut out = Vec::new();
    for (i, it) in tier.items.iter().enumerate() {
        let mut score = 0.0;
        let mut matched = 0;
        for t in q {
            let f = *it.tf.get(t).unwrap_or(&0) as f64;
            if f == 0.0 {
                continue;
            }
            matched += 1;
            let df = *tier.df.get(t).unwrap_or(&0) as f64;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            score += idf * f * (k1 + 1.0) / (f + k1 * (1.0 - b + b * it.len / tier.avgdl));
        }
        if matched >= need && score > 0.0 {
            if it.pinned {
                score *= 1.15;
            }
            out.push((i, score));
        }
    }
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn to_hit(it: &Item, score: f64, max_chars: usize) -> BasinHit {
    let mut text = it.text.trim().to_string();
    if text.chars().count() > max_chars {
        text = text.chars().take(max_chars).collect::<String>() + " […]";
    }
    BasinHit {
        tier: it.tier.to_string(),
        id: it.id.clone(),
        doc_id: it.doc_id.clone(),
        doc_path: it.doc_path.clone(),
        page: it.page,
        heading: it.heading.clone(),
        text,
        score: (score * 1000.0).round() / 1000.0,
    }
}

/// Curated passages first (up to k_t1), then raw chunks to fill to k,
/// within a total character budget so the prompt stays inside the model's context.
pub fn search(idx: &Index, query: &str, k: usize, char_budget: usize) -> Vec<BasinHit> {
    let mut q: Vec<String> = tokenize(query);
    let mut seen = HashSet::new();
    q.retain(|t| seen.insert(t.clone()));
    if q.is_empty() {
        return vec![];
    }
    let per_hit = 1400usize;
    let k_t1 = (k + 1) / 2;
    let mut hits: Vec<BasinHit> = Vec::new();
    let mut used = 0usize;
    let mut taken: Vec<(String, i64, i64, i64)> = Vec::new(); // doc, page, start, end
    let mut texts: HashSet<String> = HashSet::new();

    let t1 = score_tier(&idx.t1, &q);
    let best1 = t1.first().map(|x| x.1).unwrap_or(0.0);
    for (i, s) in t1.into_iter() {
        if hits.len() >= k_t1 || s < best1 * 0.35 {
            break;
        }
        let it = &idx.t1.items[i];
        let h = to_hit(it, s, per_hit);
        if used + h.text.len() > char_budget {
            break;
        }
        used += h.text.len();
        taken.push((it.doc_id.clone(), it.page, it.start, it.end));
        texts.insert(it.text.trim().to_string());
        hits.push(h);
    }

    let t2 = score_tier(&idx.t2, &q);
    let best2 = t2.first().map(|x| x.1).unwrap_or(0.0);
    for (i, s) in t2.into_iter() {
        if hits.len() >= k || s < best2 * 0.35 {
            break;
        }
        let it = &idx.t2.items[i];
        let overlaps = taken.iter().any(|(d, p, a, b)| d == &it.doc_id && *p == it.page && it.start < *b && *a < it.end);
        if overlaps || !texts.insert(it.text.trim().to_string()) {
            continue;
        }
        let h = to_hit(it, s, per_hit);
        if used + h.text.len() > char_budget {
            break;
        }
        used += h.text.len();
        taken.push((it.doc_id.clone(), it.page, it.start, it.end));
        hits.push(h);
    }
    hits
}

// ------------------------------------------------------------------ cache
struct Cached {
    dir: PathBuf,
    stamp: (Option<SystemTime>, Option<SystemTime>),
    index: Index,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

fn stamp(dir: &Path) -> (Option<SystemTime>, Option<SystemTime>) {
    let m = |f: &str| std::fs::metadata(dir.join(f)).and_then(|m| m.modified()).ok();
    (m("segments.jsonl"), m("chunks.jsonl"))
}

pub fn default_dir() -> PathBuf {
    if let Ok(p) = std::env::var("RI1_BASIN_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new(&home).join("Desktop").join("RI1-BASIN").join("basin")
}

/// Search with an index cached in memory and reloaded whenever the Basin is rebuilt.
pub fn cached_search(dir: &Path, query: &str, k: usize, char_budget: usize) -> Result<(Vec<BasinHit>, usize, usize), String> {
    let st = stamp(dir);
    let mut guard = CACHE.lock().map_err(|_| "basin cache lock poisoned".to_string())?;
    let stale = match guard.as_ref() {
        Some(c) => c.dir != dir || c.stamp != st,
        None => true,
    };
    if stale {
        let index = load(dir)?;
        *guard = Some(Cached { dir: dir.to_path_buf(), stamp: st, index });
    }
    let c = guard.as_ref().expect("cache just filled");
    Ok((search(&c.index, query, k, char_budget), c.index.t1.items.len(), c.index.t2.items.len()))
}
