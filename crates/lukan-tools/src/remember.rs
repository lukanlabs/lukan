use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use async_trait::async_trait;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use lukan_core::{config::ProjectConfig, models::tools::ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::{error, info};

use crate::{Tool, ToolContext};

// ── Retrieval Configuration ───────────────────────────────────────────

const MEMORY_RETRIEVAL_ENV: &str = "LUKAN_MEMORY_RETRIEVAL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRetrievalMode {
    Keyword,
    Bm25,
    Hybrid,
}

impl MemoryRetrievalMode {
    pub fn from_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "keyword" => Some(Self::Keyword),
            "bm25" => Some(Self::Bm25),
            "hybrid" => Some(Self::Hybrid),
            _ => None,
        }
    }

    pub fn from_env() -> Self {
        env::var(MEMORY_RETRIEVAL_ENV)
            .ok()
            .and_then(|value| Self::from_value(&value))
            .unwrap_or(Self::Keyword)
    }

    pub async fn from_config_or_env(cwd: &Path) -> Self {
        if let Ok(value) = env::var(MEMORY_RETRIEVAL_ENV)
            && let Some(mode) = Self::from_value(&value)
        {
            return mode;
        }

        if let Ok(Some((_, config))) = ProjectConfig::load(cwd).await
            && let Some(value) = config.memory_retrieval.as_deref()
            && let Some(mode) = Self::from_value(value)
        {
            return mode;
        }

        Self::Keyword
    }
}

// ── Data Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MemoryFrontmatter {
    pub tags: Vec<String>,
    pub summary: String,
    pub memory_type: String,
    pub importance: String,
    pub related: Vec<String>,
    pub created: String,
    pub updated: String,
    pub code_refs: Vec<String>,
    pub index: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryFile {
    pub filename: String,
    pub path: PathBuf,
    pub frontmatter: MemoryFrontmatter,
    /// Markdown body after the frontmatter (truncated for reranking).
    pub body: String,
}

// ── Frontmatter Parsing ─────────────────────────────────────────────

/// Parse YAML frontmatter from a markdown file.
/// Expects `---\n...\n---` at the start of the file.
pub fn parse_memory_frontmatter(content: &str) -> Option<MemoryFrontmatter> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("\n---")?;
    let yaml_block = &rest[..end];

    let mut tags = Vec::new();
    let mut summary = String::new();
    let mut memory_type = String::from("context");
    let mut importance = String::from("medium");
    let mut related = Vec::new();
    let mut created = String::new();
    let mut updated = String::new();
    let mut code_refs = Vec::new();
    let mut index = Vec::new();
    let mut in_index = false;
    let mut in_code_refs = false;

    for line in yaml_block.lines() {
        let trimmed = line.trim();

        // Handle multi-line list entries (index, code_refs)
        if in_index || in_code_refs {
            if let Some(val) = trimmed.strip_prefix("- ") {
                let val = val.trim_matches('"').trim_matches('\'').to_string();
                if in_index {
                    index.push(val);
                } else {
                    code_refs.push(val);
                }
                continue;
            } else if let Some(val) = trimmed.strip_prefix('-') {
                let val = val.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    if in_index {
                        index.push(val.to_string());
                    } else {
                        code_refs.push(val.to_string());
                    }
                }
                continue;
            } else {
                in_index = false;
                in_code_refs = false;
            }
        }

        if let Some(val) = trimmed.strip_prefix("tags:") {
            tags = parse_bracket_list(val);
        } else if let Some(val) = trimmed.strip_prefix("summary:") {
            summary = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = trimmed.strip_prefix("type:") {
            memory_type = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("importance:") {
            importance = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("related:") {
            related = parse_bracket_list(val);
        } else if let Some(val) = trimmed.strip_prefix("created:") {
            created = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("updated:") {
            updated = val.trim().to_string();
        } else if trimmed.starts_with("code_refs:") {
            let inline = trimmed.strip_prefix("code_refs:").unwrap_or("").trim();
            if inline.starts_with('[') {
                code_refs = parse_bracket_list(inline);
            } else {
                in_code_refs = true;
            }
        } else if trimmed.starts_with("index:") {
            in_index = true;
        }
    }

    if summary.is_empty() {
        return None;
    }

    Some(MemoryFrontmatter {
        tags,
        summary,
        memory_type,
        importance,
        related,
        created,
        updated,
        code_refs,
        index,
    })
}

/// Extract the markdown body after the YAML frontmatter closing `---`.
/// Returns the content trimmed, or empty string if no body.
pub fn extract_body(content: &str) -> String {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return content.to_string();
    }
    let rest = &content[3..];
    if let Some(end) = rest.find("\n---") {
        let after = &rest[end + 4..]; // skip past "\n---"
        after.trim().to_string()
    } else {
        String::new()
    }
}

/// Parse `[item1, item2, item3]` into a Vec<String>.
fn parse_bracket_list(val: &str) -> Vec<String> {
    let val = val.trim();
    let inner = val
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(val);
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ── Discovery ───────────────────────────────────────────────────────

/// Discover all structured memory files in `.lukan/memories/`.
/// Excludes `MEMORY.md` (behavior profile, not a structured memory).
pub async fn discover_memory_files(cwd: &Path) -> Vec<MemoryFile> {
    let memories_dir = cwd.join(".lukan").join("memories");
    let mut files = Vec::new();

    let mut entries = match tokio::fs::read_dir(&memories_dir).await {
        Ok(e) => e,
        Err(_) => return files,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip behavior profile, hidden files, non-markdown
        if filename == "MEMORY.md" || filename.starts_with('.') || !filename.ends_with(".md") {
            continue;
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(frontmatter) = parse_memory_frontmatter(&content) {
            let body = extract_body(&content);
            files.push(MemoryFile {
                filename,
                path,
                frontmatter,
                body,
            });
        }
    }

    // Sort by importance (high first), then by filename
    files.sort_by(|a, b| {
        let imp_order = |s: &str| match s {
            "high" => 0,
            "medium" => 1,
            "low" => 2,
            _ => 3,
        };
        imp_order(&a.frontmatter.importance)
            .cmp(&imp_order(&b.frontmatter.importance))
            .then_with(|| a.filename.cmp(&b.filename))
    });

    files
}

// ── Search ──────────────────────────────────────────────────────────

/// Maximum candidates from keyword stage before reranking.
const KEYWORD_CANDIDATES: usize = 20;

fn keyword_query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

fn keyword_score_memory(mf: &MemoryFile, query_terms: &[String]) -> usize {
    let mut score = 0usize;
    let fm = &mf.frontmatter;

    for term in query_terms {
        if fm.tags.iter().any(|t| t.to_lowercase().contains(term)) {
            score += 3;
        }
        if fm.summary.to_lowercase().contains(term) {
            score += 2;
        }
        if fm.related.iter().any(|r| r.to_lowercase().contains(term)) {
            score += 1;
        }
        if fm.memory_type.to_lowercase().contains(term) {
            score += 1;
        }
        if fm.index.iter().any(|i| i.to_lowercase().contains(term)) {
            score += 1;
        }
    }

    score
}

fn keyword_rank_memories(
    memories: impl IntoIterator<Item = MemoryFile>,
    query: &str,
    limit: usize,
) -> Vec<(usize, MemoryFile)> {
    let query_terms = keyword_query_terms(query);
    let mut scored: Vec<(usize, MemoryFile)> = memories
        .into_iter()
        .filter_map(|mf| {
            let score = keyword_score_memory(&mf, &query_terms);
            if score > 0 { Some((score, mf)) } else { None }
        })
        .collect();

    scored.sort_by_key(|x| std::cmp::Reverse(x.0));
    scored.truncate(limit);
    scored
}

fn tokenize_bm25(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| term.len() > 1)
        .map(ToString::to_string)
        .collect()
}

fn push_repeated(parts: &mut Vec<String>, text: String, times: usize) {
    if text.trim().is_empty() {
        return;
    }
    for _ in 0..times {
        parts.push(text.clone());
    }
}

fn build_bm25_text(mf: &MemoryFile) -> String {
    let fm = &mf.frontmatter;
    let mut parts = Vec::new();

    push_repeated(&mut parts, fm.tags.join(" "), 3);
    push_repeated(&mut parts, fm.summary.clone(), 2);
    push_repeated(&mut parts, fm.index.join(" "), 2);
    push_repeated(&mut parts, fm.related.join(" "), 1);
    push_repeated(&mut parts, fm.memory_type.clone(), 1);
    push_repeated(&mut parts, mf.body.clone(), 1);

    parts.join(" ")
}

fn bm25_field_boost(mf: &MemoryFile, query_terms: &[String]) -> f32 {
    let fm = &mf.frontmatter;
    let mut boost = 0.0;

    for term in query_terms {
        if fm.tags.iter().any(|t| t.to_lowercase().contains(term)) {
            boost += 1.5;
        }
        if fm.summary.to_lowercase().contains(term) {
            boost += 0.75;
        }
        if fm.index.iter().any(|i| i.to_lowercase().contains(term)) {
            boost += 0.5;
        }
        if fm.related.iter().any(|r| r.to_lowercase().contains(term)) {
            boost += 0.25;
        }
        if fm.memory_type.to_lowercase().contains(term) {
            boost += 0.25;
        }
    }

    boost
}

struct Bm25Doc {
    memory: MemoryFile,
    term_freqs: HashMap<String, usize>,
    len: usize,
}

fn bm25_rank_memories(
    memories: impl IntoIterator<Item = MemoryFile>,
    query: &str,
    limit: usize,
) -> Vec<(f32, MemoryFile)> {
    let query_terms = tokenize_bm25(query);
    if query_terms.is_empty() {
        return Vec::new();
    }

    let mut docs = Vec::new();
    let mut doc_freqs: HashMap<String, usize> = HashMap::new();

    for memory in memories {
        let tokens = tokenize_bm25(&build_bm25_text(&memory));
        let mut term_freqs = HashMap::new();
        for token in tokens {
            *term_freqs.entry(token).or_insert(0) += 1;
        }

        for term in term_freqs.keys().collect::<HashSet<_>>() {
            *doc_freqs.entry(term.clone()).or_insert(0) += 1;
        }

        let len = term_freqs.values().sum();
        docs.push(Bm25Doc {
            memory,
            term_freqs,
            len,
        });
    }

    if docs.is_empty() {
        return Vec::new();
    }

    let n_docs = docs.len() as f32;
    let avg_doc_len = docs.iter().map(|doc| doc.len as f32).sum::<f32>() / n_docs;
    let k1 = 1.2;
    let b = 0.75;

    let mut scored = Vec::new();
    for doc in docs {
        let mut score = 0.0;
        for term in &query_terms {
            let Some(&tf) = doc.term_freqs.get(term) else {
                continue;
            };
            let df = *doc_freqs.get(term).unwrap_or(&0) as f32;
            let idf = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
            let tf = tf as f32;
            let doc_len = doc.len as f32;
            let norm = tf + k1 * (1.0 - b + b * doc_len / avg_doc_len.max(1.0));
            score += idf * (tf * (k1 + 1.0)) / norm;
        }

        score += bm25_field_boost(&doc.memory, &query_terms);
        if score > 0.0 {
            scored.push((score, doc.memory));
        }
    }

    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(limit);
    scored
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RetrievalSource {
    Keyword,
    Bm25,
    Embedding,
}

#[derive(Debug, Clone)]
struct RetrievalCandidate {
    memory: MemoryFile,
    #[allow(dead_code)]
    sources: Vec<RetrievalSource>,
}

#[derive(Debug, Clone, Default)]
struct RetrievalStats {
    candidate_count: usize,
    keyword_candidates: usize,
    bm25_candidates: usize,
    embedding_candidates: usize,
    embedding_status: Option<EmbeddingStatus>,
}

#[derive(Debug, Clone)]
struct EmbeddingStatus {
    attempted: bool,
    index_documents: Option<usize>,
    raw_candidates: usize,
    error: Option<String>,
}

impl EmbeddingStatus {
    fn describe(&self) -> String {
        if !self.attempted {
            return "not-attempted".to_string();
        }
        let index = self
            .index_documents
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        match &self.error {
            Some(error) => format!(
                "failed(index_docs={index}, raw_candidates={}, error={error})",
                self.raw_candidates
            ),
            None => format!(
                "ok(index_docs={index}, raw_candidates={})",
                self.raw_candidates
            ),
        }
    }
}

const EMBEDDING_INDEX_VERSION: u32 = 1;
const EMBEDDING_MODEL_NAME: &str = "fastembed/multilingual-e5-small";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryEmbeddingIndex {
    version: u32,
    embedding_model: String,
    documents: Vec<MemoryEmbeddingDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryEmbeddingDocument {
    filename: String,
    relative_path: String,
    content_hash: String,
    embedding: Vec<f32>,
}

trait MemoryEmbedder {
    fn model_name(&self) -> &str;
    fn embed(&mut self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
}

struct FastEmbedMemoryEmbedder;

impl MemoryEmbedder for FastEmbedMemoryEmbedder {
    fn model_name(&self) -> &str {
        EMBEDDING_MODEL_NAME
    }

    fn embed(&mut self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        static EMBEDDER: OnceLock<Result<Mutex<TextEmbedding>, String>> = OnceLock::new();
        let result = EMBEDDER.get_or_init(|| {
            info!("Loading memory embedding model (first use)...");
            TextEmbedding::try_new(
                TextInitOptions::new(EmbeddingModel::MultilingualE5Small)
                    .with_show_download_progress(true),
            )
            .map(Mutex::new)
            .map_err(|e| e.to_string())
        });
        let mutex = match result {
            Ok(mutex) => mutex,
            Err(e) => anyhow::bail!("failed to load memory embedding model: {e}"),
        };
        let mut embedder = match mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        embedder.embed(texts, Some(16))
    }
}

fn embedding_index_path(cwd: &Path) -> PathBuf {
    cwd.join(".lukan")
        .join("cache")
        .join("memories")
        .join("index.json")
}

fn build_embedding_text(mf: &MemoryFile) -> String {
    let fm = &mf.frontmatter;
    let mut text = String::new();
    text.push_str("Summary: ");
    text.push_str(&fm.summary);
    if !fm.tags.is_empty() {
        text.push_str("\nTags: ");
        text.push_str(&fm.tags.join(", "));
    }
    if !fm.related.is_empty() {
        text.push_str("\nRelated: ");
        text.push_str(&fm.related.join(", "));
    }
    if !fm.index.is_empty() {
        text.push_str("\nSections: ");
        text.push_str(&fm.index.join("; "));
    }
    if !mf.body.is_empty() {
        text.push_str("\nBody:\n");
        text.push_str(&mf.body);
    }
    text
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn read_embedding_index(cwd: &Path) -> Option<MemoryEmbeddingIndex> {
    let path = embedding_index_path(cwd);
    let content = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&content).ok()
}

async fn write_embedding_index(cwd: &Path, index: &MemoryEmbeddingIndex) -> anyhow::Result<()> {
    let path = embedding_index_path(cwd);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = serde_json::to_string_pretty(index)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

async fn build_or_update_embedding_index_with_embedder(
    cwd: &Path,
    memories: &[MemoryFile],
    embedder: &mut impl MemoryEmbedder,
) -> anyhow::Result<MemoryEmbeddingIndex> {
    let existing = read_embedding_index(cwd).await.filter(|index| {
        index.version == EMBEDDING_INDEX_VERSION && index.embedding_model == embedder.model_name()
    });
    let mut existing_docs: HashMap<String, MemoryEmbeddingDocument> = existing
        .map(|index| {
            index
                .documents
                .into_iter()
                .map(|doc| (doc.filename.clone(), doc))
                .collect()
        })
        .unwrap_or_default();

    let mut documents = Vec::new();
    let mut pending = Vec::new();

    for memory in memories {
        let text = build_embedding_text(memory);
        let hash = content_hash(&text);
        if let Some(existing) = existing_docs.remove(&memory.filename)
            && existing.content_hash == hash
        {
            documents.push(existing);
            continue;
        }
        pending.push((memory, text, hash));
    }

    if !pending.is_empty() {
        let texts: Vec<String> = pending.iter().map(|(_, text, _)| text.clone()).collect();
        let embeddings = embedder.embed(&texts)?;
        for ((memory, _, hash), embedding) in pending.into_iter().zip(embeddings) {
            documents.push(MemoryEmbeddingDocument {
                filename: memory.filename.clone(),
                relative_path: format!(".lukan/memories/{}", memory.filename),
                content_hash: hash,
                embedding,
            });
        }
    }

    documents.sort_by(|a, b| a.filename.cmp(&b.filename));
    let index = MemoryEmbeddingIndex {
        version: EMBEDDING_INDEX_VERSION,
        embedding_model: embedder.model_name().to_string(),
        documents,
    };
    write_embedding_index(cwd, &index).await?;
    Ok(index)
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for (left, right) in a.iter().zip(b.iter()) {
        dot += left * right;
        norm_a += left * left;
        norm_b += right * right;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

fn rank_embeddings(
    index: &MemoryEmbeddingIndex,
    memories: &[MemoryFile],
    query_embedding: &[f32],
    limit: usize,
) -> Vec<(f32, MemoryFile)> {
    let memory_by_filename: HashMap<&str, &MemoryFile> = memories
        .iter()
        .map(|memory| (memory.filename.as_str(), memory))
        .collect();
    let mut scored = Vec::new();

    for doc in &index.documents {
        let score = cosine_similarity(query_embedding, &doc.embedding);
        if score <= 0.0 {
            continue;
        }
        if let Some(memory) = memory_by_filename.get(doc.filename.as_str()) {
            scored.push((score, (*memory).clone()));
        }
    }

    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(limit);
    scored
}

fn reciprocal_rank_fusion(rank: usize) -> f32 {
    1.0 / (60.0 + rank as f32 + 1.0)
}

fn fuse_retrieval_candidates_with_scores(
    keyword: Vec<(usize, MemoryFile)>,
    bm25: Vec<(f32, MemoryFile)>,
    embedding: Vec<(f32, MemoryFile)>,
    limit: usize,
) -> Vec<(f32, RetrievalCandidate)> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut memories: HashMap<String, MemoryFile> = HashMap::new();
    let mut sources: HashMap<String, Vec<RetrievalSource>> = HashMap::new();

    for (rank, (_, memory)) in keyword.into_iter().enumerate() {
        let key = memory.filename.clone();
        memories.entry(key.clone()).or_insert(memory);
        *scores.entry(key.clone()).or_insert(0.0) += reciprocal_rank_fusion(rank);
        sources
            .entry(key)
            .or_default()
            .push(RetrievalSource::Keyword);
    }

    for (rank, (_, memory)) in bm25.into_iter().enumerate() {
        let key = memory.filename.clone();
        memories.entry(key.clone()).or_insert(memory);
        *scores.entry(key.clone()).or_insert(0.0) += reciprocal_rank_fusion(rank);
        let entry = sources.entry(key).or_default();
        if !entry.contains(&RetrievalSource::Bm25) {
            entry.push(RetrievalSource::Bm25);
        }
    }

    for (rank, (_, memory)) in embedding.into_iter().enumerate() {
        let key = memory.filename.clone();
        memories.entry(key.clone()).or_insert(memory);
        *scores.entry(key.clone()).or_insert(0.0) += reciprocal_rank_fusion(rank);
        let entry = sources.entry(key).or_default();
        if !entry.contains(&RetrievalSource::Embedding) {
            entry.push(RetrievalSource::Embedding);
        }
    }

    let mut fused: Vec<(f32, RetrievalCandidate)> = memories
        .into_iter()
        .filter_map(|(key, memory)| {
            Some((
                *scores.get(&key)?,
                RetrievalCandidate {
                    memory,
                    sources: sources.remove(&key).unwrap_or_default(),
                },
            ))
        })
        .collect();

    fused.sort_by(|a, b| b.0.total_cmp(&a.0));
    fused.truncate(limit);
    fused
}

#[cfg(test)]
fn fuse_retrieval_candidates(
    keyword: Vec<(usize, MemoryFile)>,
    bm25: Vec<(f32, MemoryFile)>,
    embedding: Vec<(f32, MemoryFile)>,
    limit: usize,
) -> Vec<RetrievalCandidate> {
    fuse_retrieval_candidates_with_scores(keyword, bm25, embedding, limit)
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}

fn retrieve_memory_candidates_with_scores(
    memories: Vec<MemoryFile>,
    query: &str,
    mode: MemoryRetrievalMode,
    limit: usize,
) -> (Vec<(f32, RetrievalCandidate)>, RetrievalStats) {
    let keyword = keyword_rank_memories(memories.clone(), query, limit);
    let keyword_candidates = keyword.len();
    match mode {
        MemoryRetrievalMode::Keyword => {
            let candidates: Vec<(f32, RetrievalCandidate)> = keyword
                .into_iter()
                .enumerate()
                .map(|(rank, (_, memory))| {
                    (
                        reciprocal_rank_fusion(rank),
                        RetrievalCandidate {
                            memory,
                            sources: vec![RetrievalSource::Keyword],
                        },
                    )
                })
                .collect();
            let candidate_count = candidates.len();
            (
                candidates,
                RetrievalStats {
                    candidate_count,
                    keyword_candidates,
                    ..Default::default()
                },
            )
        }
        MemoryRetrievalMode::Bm25 | MemoryRetrievalMode::Hybrid => {
            let bm25 = bm25_rank_memories(memories, query, limit);
            let bm25_candidates = bm25.len();
            let candidates =
                fuse_retrieval_candidates_with_scores(keyword, bm25, Vec::new(), limit);
            let candidate_count = candidates.len();
            (
                candidates,
                RetrievalStats {
                    candidate_count,
                    keyword_candidates,
                    bm25_candidates,
                    ..Default::default()
                },
            )
        }
    }
}

#[cfg(test)]
fn retrieve_memory_candidates(
    memories: Vec<MemoryFile>,
    query: &str,
    mode: MemoryRetrievalMode,
    limit: usize,
) -> Vec<RetrievalCandidate> {
    retrieve_memory_candidates_with_scores(memories, query, mode, limit)
        .0
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}

async fn retrieve_memory_candidates_hybrid_with_embedder(
    cwd: &Path,
    memories: Vec<MemoryFile>,
    query: &str,
    limit: usize,
    embedder: &mut impl MemoryEmbedder,
) -> (Vec<RetrievalCandidate>, RetrievalStats) {
    let keyword = keyword_rank_memories(memories.clone(), query, limit);
    let keyword_candidates = keyword.len();
    let bm25 = bm25_rank_memories(memories.clone(), query, limit);
    let bm25_candidates = bm25.len();
    let mut embedding_status = EmbeddingStatus {
        attempted: true,
        index_documents: None,
        raw_candidates: 0,
        error: None,
    };
    let embedding =
        match build_or_update_embedding_index_with_embedder(cwd, &memories, embedder).await {
            Ok(index) => {
                embedding_status.index_documents = Some(index.documents.len());
                match embedder.embed(&[query.to_string()]) {
                    Ok(mut embeddings) => embeddings
                        .pop()
                        .map(|query_embedding| {
                            rank_embeddings(&index, &memories, &query_embedding, limit)
                        })
                        .unwrap_or_default(),
                    Err(e) => {
                        let message = e.to_string();
                        error!("Memory query embedding failed: {message}");
                        embedding_status.error = Some(message);
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                let message = e.to_string();
                error!("Memory embedding index update failed: {message}");
                embedding_status.error = Some(message);
                Vec::new()
            }
        };
    embedding_status.raw_candidates = embedding.len();
    let embedding_candidates = embedding.len();

    let fused = fuse_retrieval_candidates_with_scores(keyword, bm25, embedding, limit);
    let candidate_count = fused.len();
    let candidates = fused.into_iter().map(|(_, candidate)| candidate).collect();
    (
        candidates,
        RetrievalStats {
            candidate_count,
            keyword_candidates,
            bm25_candidates,
            embedding_candidates,
            embedding_status: Some(embedding_status),
        },
    )
}

async fn retrieve_memory_candidates_with_mode(
    cwd: &Path,
    memories: Vec<MemoryFile>,
    query: &str,
    mode: MemoryRetrievalMode,
    limit: usize,
) -> (Vec<RetrievalCandidate>, RetrievalStats) {
    if mode != MemoryRetrievalMode::Hybrid {
        let (scored, stats) = retrieve_memory_candidates_with_scores(memories, query, mode, limit);
        return (
            scored.into_iter().map(|(_, candidate)| candidate).collect(),
            stats,
        );
    }

    let mut embedder = FastEmbedMemoryEmbedder;
    retrieve_memory_candidates_hybrid_with_embedder(cwd, memories, query, limit, &mut embedder)
        .await
}

#[cfg(test)]
fn retrieve_memory_candidates_hybrid_with_embedding_scores(
    memories: Vec<MemoryFile>,
    query: &str,
    embedding: Vec<(f32, MemoryFile)>,
    limit: usize,
) -> Vec<(f32, RetrievalCandidate)> {
    let keyword = keyword_rank_memories(memories.clone(), query, limit);
    let bm25 = bm25_rank_memories(memories, query, limit);
    fuse_retrieval_candidates_with_scores(keyword, bm25, embedding, limit)
}

/// Search memory files using two-stage retrieval.
/// Returns `(results, retrieval stats)` so callers can report reranking stats.
async fn search_memory_files_with_stats(
    cwd: &Path,
    query: &str,
) -> (Vec<MemoryFile>, RetrievalStats) {
    let (results, stats) = search_memory_files_inner(cwd, query).await;
    (results, stats)
}

/// Search memory files — convenience wrapper that discards stats.
pub async fn search_memory_files(cwd: &Path, query: &str) -> Vec<MemoryFile> {
    search_memory_files_inner(cwd, query).await.0
}

/// Two-stage retrieval:
/// 1. Configured candidate retrieval (keyword, BM25, or hybrid embeddings)
/// 2. Semantic reranking (cross-encoder) to pick the best matches
///
/// Returns `(results, retrieval stats)`.
async fn search_memory_files_inner(cwd: &Path, query: &str) -> (Vec<MemoryFile>, RetrievalStats) {
    let all = discover_memory_files(cwd).await;
    let mode = MemoryRetrievalMode::from_config_or_env(cwd).await;
    let (candidates, stats) =
        retrieve_memory_candidates_with_mode(cwd, all, query, mode, KEYWORD_CANDIDATES).await;

    let candidates: Vec<MemoryFile> = candidates
        .into_iter()
        .map(|candidate| candidate.memory)
        .collect();

    if candidates.is_empty() {
        return (candidates, stats);
    }

    // Stage 2: semantic reranking (metadata + body content)
    let documents: Vec<String> = candidates
        .iter()
        .map(|mf| {
            crate::reranker::build_document(
                &mf.frontmatter.summary,
                &mf.frontmatter.tags,
                &mf.frontmatter.related,
                &mf.frontmatter.index,
                &mf.body,
            )
        })
        .collect();

    let reranked_indices =
        crate::reranker::rerank(query, &documents, crate::reranker::RERANK_TOP_N);

    let results = reranked_indices
        .into_iter()
        .filter_map(|i| candidates.get(i).cloned())
        .collect();

    (results, stats)
}

// ── Prompt Helpers ──────────────────────────────────────────────────

/// Generate a compact summary listing for injection into the system prompt.
/// Caps at 20 entries.
pub async fn get_memory_summaries_for_prompt(cwd: &Path) -> Option<String> {
    let files = discover_memory_files(cwd).await;
    if files.is_empty() {
        return None;
    }

    let mut section = String::from(
        "## Project Memories\n\
         IMPORTANT: Before starting any task that modifies code, creates files, or makes decisions, \
         call the Remember tool with keywords related to the task. Past decisions, mistakes, and \
         lessons are stored here and MUST be consulted to avoid repeating errors.\n\n",
    );

    let cap = files.len().min(20);
    for mf in &files[..cap] {
        let fm = &mf.frontmatter;
        let date = if !fm.updated.is_empty() {
            &fm.updated
        } else if !fm.created.is_empty() {
            &fm.created
        } else {
            "?"
        };
        section.push_str(&format!(
            "- **{}** [{}|{}|{}]: {}\n",
            mf.filename, fm.memory_type, fm.importance, date, fm.summary,
        ));
    }

    if files.len() > 20 {
        section.push_str(&format!(
            "\n({} more memories available via Remember tool)\n",
            files.len() - 20
        ));
    }

    Some(section)
}

/// Format all frontmatters for the LLM memory update prompt.
pub fn format_frontmatters_for_llm(files: &[MemoryFile]) -> String {
    if files.is_empty() {
        return String::from("(no existing memory files)");
    }
    files
        .iter()
        .map(|mf| {
            let fm = &mf.frontmatter;
            let date = if !fm.updated.is_empty() {
                format!(" | updated: {}", fm.updated)
            } else if !fm.created.is_empty() {
                format!(" | created: {}", fm.created)
            } else {
                String::new()
            };
            let refs = if !fm.code_refs.is_empty() {
                format!(" | code_refs: [{}]", fm.code_refs.join(", "))
            } else {
                String::new()
            };
            format!(
                "[{}] tags: [{}] | type: {} | importance: {} | summary: {}{}{}",
                mf.filename,
                fm.tags.join(", "),
                fm.memory_type,
                fm.importance,
                fm.summary,
                date,
                refs,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check if MEMORY.md (behavior profile) exists.
pub async fn has_behavior_profile(cwd: &Path) -> bool {
    tokio::fs::metadata(cwd.join(".lukan").join("memories").join("MEMORY.md"))
        .await
        .is_ok()
}

/// Read MEMORY.md (behavior profile) content.
pub async fn read_behavior_profile(cwd: &Path) -> Option<String> {
    tokio::fs::read_to_string(cwd.join(".lukan").join("memories").join("MEMORY.md"))
        .await
        .ok()
        .filter(|s| !s.trim().is_empty())
}

// ── Remember Tool ───────────────────────────────────────────────────

pub struct RememberTool;

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "Remember"
    }

    fn description(&self) -> &str {
        "ALWAYS call this tool BEFORE starting any task. It contains past decisions, \
         mistakes to avoid, and lessons learned from this project. Skipping this tool \
         risks repeating past errors. Query with keywords related to your task. \
         Returns matching memory summaries and index entries. \
         Use ReadFiles with line ranges to load full details if needed."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to recall (e.g. 'grep tool decisions', 'auth flow', 'error handling patterns')"
                }
            },
            "required": ["query"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn search_hint(&self) -> Option<&str> {
        Some("recall project decisions and lessons learned")
    }

    fn activity_label(&self, _input: &serde_json::Value) -> Option<String> {
        Some("Recalling memories".to_string())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: query"))?;

        // Run configured retrieval + reranker search
        let mode = MemoryRetrievalMode::from_config_or_env(&ctx.cwd).await;
        let (matches, stats) = search_memory_files_with_stats(&ctx.cwd, query).await;

        if matches.is_empty() {
            let mut output = format!(
                "No matching memories found. Retrieval stats: keyword={}, bm25={}, embeddings={}",
                stats.keyword_candidates, stats.bm25_candidates, stats.embedding_candidates,
            );
            if let Some(status) = &stats.embedding_status {
                output.push_str(&format!(" ({})", status.describe()));
            }
            output.push('.');
            return Ok(ToolResult::success(output));
        }

        let reranked = stats.candidate_count > matches.len();
        let mut output = if reranked {
            format!(
                "Found {} memories (mode: {:?}, reranked from {} retrieval candidates):\n",
                matches.len(),
                mode,
                stats.candidate_count,
            )
        } else {
            format!(
                "Found {} matching memories (mode: {:?}):\n",
                matches.len(),
                mode
            )
        };
        output.push_str(&format!(
            "Retrieval stats: keyword={}, bm25={}, embeddings={}",
            stats.keyword_candidates, stats.bm25_candidates, stats.embedding_candidates,
        ));
        if let Some(status) = &stats.embedding_status {
            output.push_str(&format!(" ({})", status.describe()));
        }
        output.push('\n');

        for mf in &matches {
            let fm = &mf.frontmatter;
            output.push_str(&format!(
                "\n--- {} [{}|{}] ---\n",
                mf.filename, fm.memory_type, fm.importance,
            ));
            output.push_str(&format!("Summary: {}\n", fm.summary));
            output.push_str(&format!("Tags: {}\n", fm.tags.join(", ")));

            // Show freshness info
            if !fm.updated.is_empty() {
                output.push_str(&format!("Updated: {}\n", fm.updated));
            } else if !fm.created.is_empty() {
                output.push_str(&format!("Created: {} (never updated)\n", fm.created));
            }

            // Show source files so the agent can verify
            if !fm.code_refs.is_empty() {
                output.push_str("Source files:\n");
                for r in &fm.code_refs {
                    output.push_str(&format!("  - {r}\n"));
                }
            }

            if !fm.index.is_empty() {
                output.push_str("Index:\n");
                for entry in &fm.index {
                    output.push_str(&format!("  - {entry}\n"));
                }
            }

            output.push_str(&format!("Path: {}\n", mf.path.display()));
        }

        Ok(ToolResult::success(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_memory(
        filename: &str,
        tags: &[&str],
        summary: &str,
        related: &[&str],
        memory_type: &str,
        index: &[&str],
        body: &str,
    ) -> MemoryFile {
        MemoryFile {
            filename: filename.to_string(),
            path: PathBuf::from(filename),
            frontmatter: MemoryFrontmatter {
                tags: tags.iter().map(|s| s.to_string()).collect(),
                summary: summary.to_string(),
                memory_type: memory_type.to_string(),
                importance: "medium".to_string(),
                related: related.iter().map(|s| s.to_string()).collect(),
                created: String::new(),
                updated: String::new(),
                code_refs: Vec::new(),
                index: index.iter().map(|s| s.to_string()).collect(),
            },
            body: body.to_string(),
        }
    }

    #[test]
    fn synthetic_eval_bm25_improves_body_recall_over_keyword() {
        let memories = vec![
            test_memory(
                "metadata.md",
                &["release"],
                "release workflow",
                &[],
                "context",
                &[],
                "",
            ),
            test_memory(
                "body.md",
                &[],
                "unrelated summary",
                &[],
                "context",
                &[],
                "docker hub image publish container",
            ),
        ];

        let keyword = retrieve_memory_candidates_with_scores(
            memories.clone(),
            "docker image",
            MemoryRetrievalMode::Keyword,
            20,
        );
        let bm25 = retrieve_memory_candidates_with_scores(
            memories,
            "docker image",
            MemoryRetrievalMode::Bm25,
            20,
        );

        assert!(keyword.0.is_empty());
        assert_eq!(bm25.0[0].1.memory.filename, "body.md");
    }

    #[test]
    fn synthetic_eval_hybrid_can_surface_semantic_only_candidate() {
        let lexical = test_memory(
            "lexical.md",
            &["release"],
            "release workflow",
            &[],
            "context",
            &[],
            "",
        );
        let semantic = test_memory(
            "semantic.md",
            &[],
            "unrelated wording",
            &[],
            "context",
            &[],
            "container publishing workflow",
        );
        let embedding = vec![(0.95, semantic.clone())];

        let hybrid = retrieve_memory_candidates_hybrid_with_embedding_scores(
            vec![lexical, semantic],
            "subir imagen docker",
            embedding,
            20,
        );

        assert_eq!(hybrid[0].1.memory.filename, "semantic.md");
        assert!(hybrid[0].1.sources.contains(&RetrievalSource::Embedding));
    }

    #[test]
    fn synthetic_eval_keyword_default_keeps_curated_tag_priority() {
        let tag = test_memory("tag.md", &["docker"], "unrelated", &[], "context", &[], "");
        let summary = test_memory(
            "summary.md",
            &[],
            "docker workflow",
            &[],
            "context",
            &[],
            "",
        );

        let keyword = retrieve_memory_candidates_with_scores(
            vec![summary, tag],
            "docker",
            MemoryRetrievalMode::Keyword,
            20,
        );

        assert_eq!(keyword.0[0].1.memory.filename, "tag.md");
    }

    #[tokio::test]
    async fn real_memories_eval_compares_keyword_and_bm25_on_repository_memories() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let memories = discover_memory_files(&cwd).await;
        if memories.is_empty() {
            eprintln!("No local .lukan/memories corpus found; skipping real eval");
            return;
        }

        let cases = [
            ("docker hub publishing", "build-commands.md"),
            (
                "cloud relay frontend deployment",
                "cloud-relay-deploy-workflow.md",
            ),
            ("structured memories keyword reranking", "memory-system.md"),
            (
                "bubblewrap sandbox encryption permissions",
                "security-model.md",
            ),
            (
                "settings overlay cursor focus",
                "settings-overlay-transition-focus-fix.md",
            ),
        ];

        for (query, expected) in cases {
            let keyword = retrieve_memory_candidates_with_scores(
                memories.clone(),
                query,
                MemoryRetrievalMode::Keyword,
                20,
            );
            let bm25 = retrieve_memory_candidates_with_scores(
                memories.clone(),
                query,
                MemoryRetrievalMode::Bm25,
                20,
            );
            let keyword_rank = candidate_rank(&keyword.0, expected);
            let bm25_rank = candidate_rank(&bm25.0, expected);

            eprintln!(
                "query={query:?} expected={expected} keyword_rank={keyword_rank:?} bm25_rank={bm25_rank:?}"
            );
            assert!(
                bm25_rank.is_some(),
                "BM25 should find expected memory {expected} for query {query:?}"
            );
            if let Some(keyword_rank) = keyword_rank {
                assert!(
                    bm25_rank.unwrap() <= keyword_rank,
                    "BM25 should not rank expected memory worse than keyword for query {query:?}"
                );
            }
        }
    }

    fn candidate_rank(candidates: &[(f32, RetrievalCandidate)], filename: &str) -> Option<usize> {
        candidates
            .iter()
            .position(|(_, candidate)| candidate.memory.filename == filename)
            .map(|rank| rank + 1)
    }

    struct SemanticFakeEmbedder {
        model: String,
        calls: usize,
    }

    impl SemanticFakeEmbedder {
        fn new(model: &str) -> Self {
            Self {
                model: model.to_string(),
                calls: 0,
            }
        }

        fn vector_for(text: &str) -> Vec<f32> {
            let lower = text.to_lowercase();
            if lower.contains("subir imagen") || lower.contains("container publishing") {
                vec![1.0, 0.0, 0.0]
            } else if lower.contains("settings overlay") || lower.contains("cursor focus") {
                vec![0.0, 1.0, 0.0]
            } else if lower.contains("sandbox") || lower.contains("encryption") {
                vec![0.0, 0.0, 1.0]
            } else {
                vec![0.1, 0.1, 0.1]
            }
        }
    }

    impl MemoryEmbedder for SemanticFakeEmbedder {
        fn model_name(&self) -> &str {
            &self.model
        }

        fn embed(&mut self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            self.calls += texts.len();
            Ok(texts.iter().map(|text| Self::vector_for(text)).collect())
        }
    }

    #[tokio::test]
    async fn hybrid_eval_with_fake_embeddings_surfaces_semantic_real_memory() {
        let cwd = unique_test_cwd("hybrid-real");
        let memories = vec![
            test_memory(
                "build-commands.md",
                &["lukan", "build", "release"],
                "Makefile commands and Docker Hub publishing workflow",
                &[],
                "context",
                &[],
                "container publishing workflow for Docker images",
            ),
            test_memory(
                "security-model.md",
                &["security"],
                "Sandbox and encryption architecture",
                &[],
                "context",
                &[],
                "bubblewrap permissions",
            ),
        ];
        let mut embedder = SemanticFakeEmbedder::new("semantic-fake-v1");

        let hybrid = retrieve_memory_candidates_hybrid_with_embedder(
            &cwd,
            memories,
            "subir imagen docker",
            20,
            &mut embedder,
        )
        .await;

        assert_eq!(hybrid.0[0].memory.filename, "build-commands.md");
        assert!(hybrid.0[0].sources.contains(&RetrievalSource::Embedding));
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[tokio::test]
    async fn real_embeddings_eval_compares_keyword_bm25_and_hybrid_on_repository_memories() {
        if env::var("LUKAN_RUN_REAL_EMBEDDING_EVAL").ok().as_deref() != Some("1") {
            eprintln!("Set LUKAN_RUN_REAL_EMBEDDING_EVAL=1 to run real fastembed memory eval");
            return;
        }

        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let memories = discover_memory_files(&cwd).await;
        if memories.is_empty() {
            eprintln!("No local .lukan/memories corpus found; skipping real embedding eval");
            return;
        }

        let cases = [
            ("subir imagen docker hub", "build-commands.md"),
            (
                "desplegar frontend remoto con cloudflare",
                "cloud-relay-deploy-workflow.md",
            ),
            (
                "seguridad aislamiento permisos cifrado",
                "security-model.md",
            ),
        ];

        for (query, expected) in cases {
            let keyword = retrieve_memory_candidates_with_scores(
                memories.clone(),
                query,
                MemoryRetrievalMode::Keyword,
                20,
            );
            let bm25 = retrieve_memory_candidates_with_scores(
                memories.clone(),
                query,
                MemoryRetrievalMode::Bm25,
                20,
            );
            let hybrid = retrieve_memory_candidates_with_mode(
                &cwd,
                memories.clone(),
                query,
                MemoryRetrievalMode::Hybrid,
                20,
            )
            .await;
            let hybrid_rank = hybrid
                .0
                .iter()
                .position(|candidate| candidate.memory.filename == expected)
                .map(|rank| rank + 1);

            eprintln!(
                "query={query:?} expected={expected} keyword_rank={:?} bm25_rank={:?} hybrid_rank={hybrid_rank:?}",
                candidate_rank(&keyword.0, expected),
                candidate_rank(&bm25.0, expected),
            );
        }
    }

    #[derive(Default)]
    struct FakeEmbedder {
        model: String,
        calls: usize,
    }

    impl FakeEmbedder {
        fn new(model: &str) -> Self {
            Self {
                model: model.to_string(),
                calls: 0,
            }
        }
    }

    impl MemoryEmbedder for FakeEmbedder {
        fn model_name(&self) -> &str {
            &self.model
        }

        fn embed(&mut self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            self.calls += texts.len();
            Ok(texts
                .iter()
                .map(|text| {
                    if text.contains("semantic") || text.contains("meaning") {
                        vec![1.0, 0.0]
                    } else if text.contains("deploy") {
                        vec![0.0, 1.0]
                    } else {
                        vec![0.5, 0.5]
                    }
                })
                .collect())
        }
    }

    struct FailingEmbedder;

    impl MemoryEmbedder for FailingEmbedder {
        fn model_name(&self) -> &str {
            "failing-v1"
        }

        fn embed(&mut self, _texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            anyhow::bail!("synthetic embedding failure")
        }
    }

    #[tokio::test]
    async fn hybrid_stats_report_embedding_success() {
        let cwd = unique_test_cwd("hybrid-stats-ok");
        let memories = vec![test_memory(
            "semantic.md",
            &[],
            "semantic memory",
            &[],
            "context",
            &[],
            "meaning",
        )];
        let mut embedder = FakeEmbedder::new("fake-v1");

        let (_candidates, stats) = retrieve_memory_candidates_hybrid_with_embedder(
            &cwd,
            memories,
            "semantic",
            20,
            &mut embedder,
        )
        .await;

        assert_eq!(stats.embedding_candidates, 1);
        let status = stats.embedding_status.unwrap();
        assert_eq!(status.index_documents, Some(1));
        assert_eq!(status.raw_candidates, 1);
        assert!(status.error.is_none());
        assert!(status.describe().starts_with("ok("));
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[tokio::test]
    async fn hybrid_stats_report_embedding_failure_fallback() {
        let cwd = unique_test_cwd("hybrid-stats-fail");
        let memories = vec![test_memory(
            "docker.md",
            &[],
            "docker workflow",
            &[],
            "context",
            &[],
            "docker hub image publish",
        )];
        let mut embedder = FailingEmbedder;

        let (candidates, stats) = retrieve_memory_candidates_hybrid_with_embedder(
            &cwd,
            memories,
            "docker",
            20,
            &mut embedder,
        )
        .await;

        assert!(!candidates.is_empty());
        assert_eq!(stats.embedding_candidates, 0);
        let status = stats.embedding_status.unwrap();
        assert!(
            status
                .error
                .unwrap()
                .contains("synthetic embedding failure")
        );
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    fn unique_test_cwd(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lukan-memory-{name}-{nanos}"))
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[tokio::test]
    async fn embedding_index_creates_cache_when_missing() {
        let cwd = unique_test_cwd("create");
        let memories = vec![test_memory(
            "semantic.md",
            &[],
            "semantic memory",
            &[],
            "context",
            &[],
            "meaning",
        )];
        let mut embedder = FakeEmbedder::new("fake-v1");

        let index = build_or_update_embedding_index_with_embedder(&cwd, &memories, &mut embedder)
            .await
            .unwrap();

        assert_eq!(embedder.calls, 1);
        assert_eq!(index.documents.len(), 1);
        assert!(embedding_index_path(&cwd).exists());
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[tokio::test]
    async fn embedding_index_reuses_unchanged_hashes() {
        let cwd = unique_test_cwd("reuse");
        let memories = vec![test_memory(
            "semantic.md",
            &[],
            "semantic memory",
            &[],
            "context",
            &[],
            "meaning",
        )];
        let mut embedder = FakeEmbedder::new("fake-v1");
        build_or_update_embedding_index_with_embedder(&cwd, &memories, &mut embedder)
            .await
            .unwrap();

        let mut second_embedder = FakeEmbedder::new("fake-v1");
        let index =
            build_or_update_embedding_index_with_embedder(&cwd, &memories, &mut second_embedder)
                .await
                .unwrap();

        assert_eq!(second_embedder.calls, 0);
        assert_eq!(index.documents.len(), 1);
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[tokio::test]
    async fn embedding_index_removes_deleted_memories() {
        let cwd = unique_test_cwd("delete");
        let first = test_memory("first.md", &[], "semantic", &[], "context", &[], "meaning");
        let second = test_memory("second.md", &[], "deploy", &[], "context", &[], "deploy");
        let mut embedder = FakeEmbedder::new("fake-v1");
        build_or_update_embedding_index_with_embedder(
            &cwd,
            &[first.clone(), second],
            &mut embedder,
        )
        .await
        .unwrap();

        let mut second_embedder = FakeEmbedder::new("fake-v1");
        let index =
            build_or_update_embedding_index_with_embedder(&cwd, &[first], &mut second_embedder)
                .await
                .unwrap();

        assert_eq!(index.documents.len(), 1);
        assert_eq!(index.documents[0].filename, "first.md");
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[tokio::test]
    async fn embedding_index_rebuilds_when_model_changes() {
        let cwd = unique_test_cwd("model-change");
        let memories = vec![test_memory(
            "semantic.md",
            &[],
            "semantic",
            &[],
            "context",
            &[],
            "",
        )];
        let mut embedder = FakeEmbedder::new("fake-v1");
        build_or_update_embedding_index_with_embedder(&cwd, &memories, &mut embedder)
            .await
            .unwrap();

        let mut new_embedder = FakeEmbedder::new("fake-v2");
        let index =
            build_or_update_embedding_index_with_embedder(&cwd, &memories, &mut new_embedder)
                .await
                .unwrap();

        assert_eq!(new_embedder.calls, 1);
        assert_eq!(index.embedding_model, "fake-v2");
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[tokio::test]
    async fn embedding_index_recovers_from_corrupt_cache() {
        let cwd = unique_test_cwd("corrupt");
        let path = embedding_index_path(&cwd);
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, "not json").await.unwrap();
        let memories = vec![test_memory(
            "semantic.md",
            &[],
            "semantic",
            &[],
            "context",
            &[],
            "",
        )];
        let mut embedder = FakeEmbedder::new("fake-v1");

        let index = build_or_update_embedding_index_with_embedder(&cwd, &memories, &mut embedder)
            .await
            .unwrap();

        assert_eq!(embedder.calls, 1);
        assert_eq!(index.documents.len(), 1);
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[test]
    fn embedding_cosine_similarity_ranks_related_vectors() {
        let semantic = test_memory(
            "semantic.md",
            &[],
            "semantic",
            &[],
            "context",
            &[],
            "meaning",
        );
        let deploy = test_memory("deploy.md", &[], "deploy", &[], "context", &[], "deploy");
        let index = MemoryEmbeddingIndex {
            version: EMBEDDING_INDEX_VERSION,
            embedding_model: "fake-v1".to_string(),
            documents: vec![
                MemoryEmbeddingDocument {
                    filename: "semantic.md".to_string(),
                    relative_path: ".lukan/memories/semantic.md".to_string(),
                    content_hash: "a".to_string(),
                    embedding: vec![1.0, 0.0],
                },
                MemoryEmbeddingDocument {
                    filename: "deploy.md".to_string(),
                    relative_path: ".lukan/memories/deploy.md".to_string(),
                    content_hash: "b".to_string(),
                    embedding: vec![0.0, 1.0],
                },
            ],
        };

        let ranked = rank_embeddings(&index, &[semantic, deploy], &[1.0, 0.0], 20);

        assert_eq!(ranked[0].1.filename, "semantic.md");
    }

    #[test]
    fn hybrid_fusion_includes_embedding_only_candidates() {
        let keyword = vec![(
            3,
            test_memory("keyword.md", &["deploy"], "", &[], "context", &[], ""),
        )];
        let bm25 = vec![(
            1.0,
            test_memory("bm25.md", &[], "", &[], "context", &[], "deploy"),
        )];
        let embedding = vec![(
            0.9,
            test_memory(
                "semantic.md",
                &[],
                "semantic",
                &[],
                "context",
                &[],
                "meaning",
            ),
        )];

        let fused = fuse_retrieval_candidates(keyword, bm25, embedding, 20);
        let filenames: Vec<_> = fused
            .iter()
            .map(|candidate| candidate.memory.filename.as_str())
            .collect();

        assert!(filenames.contains(&"keyword.md"));
        assert!(filenames.contains(&"bm25.md"));
        assert!(filenames.contains(&"semantic.md"));
    }

    #[test]
    fn hybrid_fusion_deduplicates_across_all_retrievers() {
        let memory = test_memory(
            "same.md",
            &["deploy"],
            "deploy",
            &[],
            "context",
            &[],
            "deploy",
        );
        let fused = fuse_retrieval_candidates(
            vec![(3, memory.clone())],
            vec![(1.0, memory.clone())],
            vec![(0.9, memory)],
            20,
        );

        assert_eq!(fused.len(), 1);
        assert!(fused[0].sources.contains(&RetrievalSource::Keyword));
        assert!(fused[0].sources.contains(&RetrievalSource::Bm25));
        assert!(fused[0].sources.contains(&RetrievalSource::Embedding));
    }

    #[test]
    fn hybrid_fusion_respects_limit() {
        let embedding: Vec<_> = (0..25)
            .map(|i| {
                (
                    1.0,
                    test_memory(
                        &format!("semantic-{i}.md"),
                        &[],
                        "semantic",
                        &[],
                        "context",
                        &[],
                        "",
                    ),
                )
            })
            .collect();
        let fused = fuse_retrieval_candidates(Vec::new(), Vec::new(), embedding, 20);

        assert_eq!(fused.len(), 20);
    }

    #[test]
    fn keyword_mode_returns_only_keyword_candidates() {
        let body_only = test_memory("body.md", &[], "unrelated", &[], "context", &[], "deploy");
        let tag_match = test_memory("tag.md", &["deploy"], "unrelated", &[], "context", &[], "");

        let candidates = retrieve_memory_candidates(
            vec![body_only, tag_match],
            "deploy",
            MemoryRetrievalMode::Keyword,
            20,
        );
        let filenames: Vec<_> = candidates
            .iter()
            .map(|candidate| candidate.memory.filename.as_str())
            .collect();

        assert_eq!(filenames, vec!["tag.md"]);
        assert_eq!(candidates[0].sources, vec![RetrievalSource::Keyword]);
    }

    #[test]
    fn bm25_mode_merges_keyword_and_bm25_candidates() {
        let body_only = test_memory("body.md", &[], "unrelated", &[], "context", &[], "deploy");
        let tag_match = test_memory("tag.md", &["deploy"], "unrelated", &[], "context", &[], "");

        let candidates = retrieve_memory_candidates(
            vec![body_only, tag_match],
            "deploy",
            MemoryRetrievalMode::Bm25,
            20,
        );
        let filenames: Vec<_> = candidates
            .iter()
            .map(|candidate| candidate.memory.filename.as_str())
            .collect();

        assert!(filenames.contains(&"tag.md"));
        assert!(filenames.contains(&"body.md"));
    }

    #[test]
    fn bm25_mode_deduplicates_memories_found_by_both_retrievers() {
        let both = test_memory(
            "both.md",
            &["deploy"],
            "deploy workflow",
            &[],
            "context",
            &[],
            "deploy body",
        );

        let candidates =
            retrieve_memory_candidates(vec![both], "deploy", MemoryRetrievalMode::Bm25, 20);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].memory.filename, "both.md");
        assert!(candidates[0].sources.contains(&RetrievalSource::Keyword));
        assert!(candidates[0].sources.contains(&RetrievalSource::Bm25));
    }

    #[test]
    fn bm25_mode_respects_fused_limit() {
        let memories = (0..25).map(|i| {
            test_memory(
                &format!("memory-{i}.md"),
                &[],
                "unrelated",
                &[],
                "context",
                &[],
                "deploy",
            )
        });

        let candidates =
            retrieve_memory_candidates(memories.collect(), "deploy", MemoryRetrievalMode::Bm25, 20);

        assert_eq!(candidates.len(), 20);
    }

    #[test]
    fn bm25_tokenize_splits_on_non_alphanumeric_and_filters_short_terms() {
        assert_eq!(
            tokenize_bm25("Cloud-relay deployment v2 A"),
            vec!["cloud", "relay", "deployment", "v2"]
        );
    }

    #[test]
    fn bm25_ranks_rare_terms_above_common_terms() {
        let rare = test_memory(
            "rare.md",
            &[],
            "unrelated",
            &[],
            "context",
            &[],
            "common zanzibar",
        );
        let common_only = test_memory(
            "common.md",
            &[],
            "unrelated",
            &[],
            "context",
            &[],
            "common common common",
        );
        let ranked = bm25_rank_memories(vec![common_only, rare], "common zanzibar", 20);

        assert_eq!(ranked[0].1.filename, "rare.md");
    }

    #[test]
    fn bm25_body_match_can_create_candidate() {
        let memory = test_memory(
            "body.md",
            &[],
            "unrelated",
            &[],
            "context",
            &[],
            "cloud relay deploy",
        );
        let ranked = bm25_rank_memories(vec![memory], "deploy", 20);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].1.filename, "body.md");
    }

    #[test]
    fn bm25_field_boost_prefers_tag_match_over_body_only_match() {
        let tag_match = test_memory("tag.md", &["deploy"], "unrelated", &[], "context", &[], "");
        let body_match = test_memory(
            "body.md",
            &[],
            "unrelated",
            &[],
            "context",
            &[],
            "deploy deploy deploy",
        );
        let ranked = bm25_rank_memories(vec![body_match, tag_match], "deploy", 20);

        assert_eq!(ranked[0].1.filename, "tag.md");
    }

    #[test]
    fn bm25_returns_no_candidates_without_shared_terms() {
        let memory = test_memory(
            "none.md",
            &["release"],
            "version bump",
            &[],
            "context",
            &[],
            "make upload",
        );
        let ranked = bm25_rank_memories(vec![memory], "cloud relay", 20);

        assert!(ranked.is_empty());
    }

    #[test]
    fn bm25_rank_respects_limit() {
        let memories = (0..25).map(|i| {
            test_memory(
                &format!("memory-{i}.md"),
                &[],
                "unrelated",
                &[],
                "context",
                &[],
                "deploy",
            )
        });

        let ranked = bm25_rank_memories(memories, "deploy", 20);

        assert_eq!(ranked.len(), 20);
    }

    #[test]
    fn keyword_terms_preserve_current_whitespace_tokenization() {
        assert_eq!(
            keyword_query_terms("Cloud relay deploy"),
            vec!["cloud", "relay", "deploy"]
        );
        assert_eq!(
            keyword_query_terms("cloud-relay deploy"),
            vec!["cloud-relay", "deploy"]
        );
    }

    #[test]
    fn keyword_score_preserves_current_field_weights() {
        let memory = test_memory(
            "memory.md",
            &["deploy"],
            "deploy workflow",
            &["deploy docs"],
            "deploy-decision",
            &["deploy checklist"],
            "deploy body is ignored by keyword search",
        );
        let terms = keyword_query_terms("deploy");

        assert_eq!(keyword_score_memory(&memory, &terms), 8);
    }

    #[test]
    fn keyword_search_ignores_body_matches() {
        let memory = test_memory(
            "body-only.md",
            &[],
            "unrelated",
            &[],
            "context",
            &[],
            "cloud relay deploy",
        );
        let terms = keyword_query_terms("deploy");

        assert_eq!(keyword_score_memory(&memory, &terms), 0);
    }

    #[test]
    fn keyword_rank_discards_zero_scores_and_orders_descending() {
        let tag_match = test_memory("tag.md", &["deploy"], "unrelated", &[], "context", &[], "");
        let summary_match = test_memory(
            "summary.md",
            &[],
            "deploy workflow",
            &[],
            "context",
            &[],
            "",
        );
        let no_match = test_memory("none.md", &[], "unrelated", &[], "context", &[], "deploy");

        let ranked = keyword_rank_memories(vec![summary_match, no_match, tag_match], "deploy", 20);
        let filenames: Vec<_> = ranked.iter().map(|(_, mf)| mf.filename.as_str()).collect();
        let scores: Vec<_> = ranked.iter().map(|(score, _)| *score).collect();

        assert_eq!(filenames, vec!["tag.md", "summary.md"]);
        assert_eq!(scores, vec![3, 2]);
    }

    #[test]
    fn keyword_rank_respects_limit() {
        let memories = (0..25).map(|i| {
            test_memory(
                &format!("memory-{i}.md"),
                &["deploy"],
                "unrelated",
                &[],
                "context",
                &[],
                "",
            )
        });

        let ranked = keyword_rank_memories(memories, "deploy", 20);

        assert_eq!(ranked.len(), 20);
    }

    #[tokio::test]
    async fn retrieval_mode_reads_project_config_when_env_is_missing() {
        let _guard = env_lock();
        let cwd = unique_test_cwd("retrieval-config");
        let lukan_dir = cwd.join(".lukan");
        tokio::fs::create_dir_all(&lukan_dir).await.unwrap();
        tokio::fs::write(
            lukan_dir.join("config.json"),
            r#"{"memoryRetrieval":"bm25"}"#,
        )
        .await
        .unwrap();
        unsafe {
            env::remove_var(MEMORY_RETRIEVAL_ENV);
        }

        assert_eq!(
            MemoryRetrievalMode::from_config_or_env(&cwd).await,
            MemoryRetrievalMode::Bm25
        );
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[tokio::test]
    async fn retrieval_mode_env_overrides_project_config() {
        let _guard = env_lock();
        let cwd = unique_test_cwd("retrieval-env-override");
        let lukan_dir = cwd.join(".lukan");
        tokio::fs::create_dir_all(&lukan_dir).await.unwrap();
        tokio::fs::write(
            lukan_dir.join("config.json"),
            r#"{"memoryRetrieval":"bm25"}"#,
        )
        .await
        .unwrap();
        unsafe {
            env::set_var(MEMORY_RETRIEVAL_ENV, "hybrid");
        }

        assert_eq!(
            MemoryRetrievalMode::from_config_or_env(&cwd).await,
            MemoryRetrievalMode::Hybrid
        );
        unsafe {
            env::remove_var(MEMORY_RETRIEVAL_ENV);
        }
        let _ = tokio::fs::remove_dir_all(cwd).await;
    }

    #[test]
    fn retrieval_mode_parses_supported_values() {
        assert_eq!(
            MemoryRetrievalMode::from_value("keyword"),
            Some(MemoryRetrievalMode::Keyword)
        );
        assert_eq!(
            MemoryRetrievalMode::from_value("bm25"),
            Some(MemoryRetrievalMode::Bm25)
        );
        assert_eq!(
            MemoryRetrievalMode::from_value("hybrid"),
            Some(MemoryRetrievalMode::Hybrid)
        );
    }

    #[test]
    fn retrieval_mode_parsing_is_case_and_whitespace_insensitive() {
        assert_eq!(
            MemoryRetrievalMode::from_value("  KEYWORD  "),
            Some(MemoryRetrievalMode::Keyword)
        );
        assert_eq!(
            MemoryRetrievalMode::from_value("Bm25"),
            Some(MemoryRetrievalMode::Bm25)
        );
        assert_eq!(
            MemoryRetrievalMode::from_value(" HYBRID\n"),
            Some(MemoryRetrievalMode::Hybrid)
        );
    }

    #[test]
    fn retrieval_mode_rejects_unknown_values() {
        assert_eq!(MemoryRetrievalMode::from_value(""), None);
        assert_eq!(MemoryRetrievalMode::from_value("semantic"), None);
        assert_eq!(MemoryRetrievalMode::from_value("bm25-hybrid"), None);
    }

    #[test]
    fn retrieval_mode_defaults_to_keyword_for_missing_or_invalid_env() {
        let _guard = env_lock();
        unsafe {
            env::remove_var(MEMORY_RETRIEVAL_ENV);
        }
        assert_eq!(
            MemoryRetrievalMode::from_env(),
            MemoryRetrievalMode::Keyword
        );

        unsafe {
            env::set_var(MEMORY_RETRIEVAL_ENV, "invalid");
        }
        assert_eq!(
            MemoryRetrievalMode::from_env(),
            MemoryRetrievalMode::Keyword
        );

        unsafe {
            env::remove_var(MEMORY_RETRIEVAL_ENV);
        }
    }

    #[test]
    fn retrieval_mode_reads_valid_env() {
        let _guard = env_lock();
        unsafe {
            env::set_var(MEMORY_RETRIEVAL_ENV, "bm25");
        }
        assert_eq!(MemoryRetrievalMode::from_env(), MemoryRetrievalMode::Bm25);

        unsafe {
            env::set_var(MEMORY_RETRIEVAL_ENV, "hybrid");
        }
        assert_eq!(MemoryRetrievalMode::from_env(), MemoryRetrievalMode::Hybrid);

        unsafe {
            env::remove_var(MEMORY_RETRIEVAL_ENV);
        }
    }
}
