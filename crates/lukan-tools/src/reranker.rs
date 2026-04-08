//! Semantic reranker powered by fastembed.
//!
//! Provides a lazily-initialized singleton that loads a lightweight
//! cross-encoder model on first use. The model runs on CPU and adds
//! ~50-100ms of latency for reranking ≤20 documents.

use std::sync::{Mutex, OnceLock};

use fastembed::{RerankInitOptions, RerankerModel, TextRerank};
use tracing::{error, info};

/// Global singleton — initialized once on first call, behind a Mutex
/// because `TextRerank::rerank` requires `&mut self`.
static RERANKER: OnceLock<Mutex<TextRerank>> = OnceLock::new();

/// Maximum number of results returned after reranking.
pub const RERANK_TOP_N: usize = 5;

/// Minimum reranker score to include a result (filters low-confidence matches).
pub const RERANK_MIN_SCORE: f32 = 0.01;

/// Initialize and return the reranker Mutex. Returns `None` if loading failed.
fn get() -> Option<&'static Mutex<TextRerank>> {
    static INIT: OnceLock<bool> = OnceLock::new();

    let success = INIT.get_or_init(|| {
        info!("Loading reranker model (first use)...");
        match TextRerank::try_new(
            RerankInitOptions::new(RerankerModel::JINARerankerV2BaseMultiligual)
                .with_show_download_progress(true),
        ) {
            Ok(model) => {
                let _ = RERANKER.set(Mutex::new(model));
                info!("Reranker model loaded successfully");
                true
            }
            Err(e) => {
                error!("Failed to load reranker model: {e}");
                false
            }
        }
    });

    if *success { RERANKER.get() } else { None }
}

/// Max characters of the body to include in the reranker document.
/// Jina v2 supports up to 1024 tokens (~3500 chars). We reserve space
/// for the metadata prefix and cap the body portion.
const BODY_CHAR_LIMIT: usize = 2000;

/// Build a reranking document from memory metadata + body content.
/// The cross-encoder scores this against the query. Including the actual
/// body gives the model real content to evaluate, not just descriptions.
pub fn build_document(
    summary: &str,
    tags: &[String],
    related: &[String],
    index: &[String],
    body: &str,
) -> String {
    let mut doc = summary.to_string();
    if !tags.is_empty() {
        doc.push_str(". Tags: ");
        doc.push_str(&tags.join(", "));
    }
    if !related.is_empty() {
        doc.push_str(". Related: ");
        doc.push_str(&related.join(", "));
    }
    if !index.is_empty() {
        doc.push_str(". Sections: ");
        doc.push_str(&index.join("; "));
    }
    if !body.is_empty() {
        doc.push_str("\n\n");
        if body.len() > BODY_CHAR_LIMIT {
            let end = body.floor_char_boundary(BODY_CHAR_LIMIT);
            doc.push_str(&body[..end]);
        } else {
            doc.push_str(body);
        }
    }
    doc
}

/// Rerank a list of documents against a query.
/// Returns indices into the original list, ordered by relevance, capped at `top_n`.
/// Falls back to identity ordering (0..len) if the model is unavailable.
pub fn rerank(query: &str, documents: &[String], top_n: usize) -> Vec<usize> {
    let Some(mutex) = get() else {
        return (0..documents.len().min(top_n)).collect();
    };

    let mut model = match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let doc_refs: Vec<&str> = documents.iter().map(|s| s.as_str()).collect();

    match model.rerank(query, doc_refs, false, Some(top_n)) {
        Ok(results) => results
            .into_iter()
            .filter(|r| r.score >= RERANK_MIN_SCORE)
            .map(|r| r.index)
            .collect(),
        Err(e) => {
            error!("Reranking failed: {e}");
            (0..documents.len().min(top_n)).collect()
        }
    }
}
