//! # assurance-core
//!
//! Core traits and types for the point-in-time factuality assurance stack.
//!
//! This crate is deliberately runtime-agnostic. It defines the contracts that
//! every other `assurance-*` crate implements:
//!
//! * [`EntityResolver`] — resolve a mention to a canonical registry identifier.
//! * [`ConfidenceScorer`] — estimate factual confidence for a resolved entity.
//! * [`RetrievalProvider`] — retrieve documents with cryptographic attestation.
//! * [`AbstentionPolicy`] — decide whether to answer, ground, or abstain.
//! * [`LlmBackend`] — a pluggable completion backend (Claude, Bedrock, etc.).
//!
//! The design goal is that a production pipeline composes these traits in a
//! single async chain and produces a [`ProvenancedOutput`] — a response whose
//! every factual span is traceable to either a retrieved document, a
//! computation, or an explicit parametric-memory acknowledgment.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

pub mod error;
pub use error::{AssuranceError, Result};

// ── Entity identity ─────────────────────────────────────────────────────────

/// A stable external identifier for a U.S. entity, drawn from a public registry.
///
/// The point of using registry IDs rather than free-text names is that they
/// collapse the long-tail ambiguity problem: "Morty" is ambiguous,
/// `ExternalId::Nmls("1429243")` is not.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExternalId {
    /// NMLS Consumer Access identifier (mortgage loan originators and companies).
    Nmls(String),
    /// SEC EDGAR Central Index Key.
    Cik(String),
    /// FDA establishment registration number.
    FdaEstablishment(String),
    /// State secretary-of-state business entity ID, qualified by USPS state code.
    StateBusiness { state: String, id: String },
    /// Opaque provider-specific identifier for future extension.
    Other { scheme: String, id: String },
}

/// A resolved candidate for an entity mention in a user prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCandidate {
    /// The surface form as it appeared in the user's prompt.
    pub surface: String,
    /// The registry IDs the resolver matched against, best match first.
    pub identifiers: Vec<ExternalId>,
    /// Popularity tier used by downstream confidence scoring.
    pub tier: PopularityTier,
}

/// Coarse popularity buckets. The tier is a prior; the confidence scorer
/// produces the posterior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopularityTier {
    /// Top ~10k global entities — dense training coverage.
    Head,
    /// Recognized but sparse — roughly the next 100k.
    Torso,
    /// Long tail — registry-only coverage; the failure region.
    LongTail,
    /// No matching registry record found.
    Unknown,
}

// ── Confidence ──────────────────────────────────────────────────────────────

/// A bounded factuality confidence estimate with an auditable rationale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Confidence {
    /// Calibrated confidence in `[0.0, 1.0]`.
    pub score: f64,
    /// Individual signals that fed into the score (tier, retrieval hits, etc.).
    pub signals: Vec<ConfidenceSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceSignal {
    pub name: String,
    pub weight: f64,
    pub value: f64,
}

// ── Retrieval & attestation ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    pub text: String,
    pub entity: Option<EntityCandidate>,
    pub index: IndexVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexVersion {
    pub name: String,
    pub version: String,
    pub built_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedDocument {
    pub doc_id: String,
    pub source_url: Option<String>,
    pub content: String,
    /// BLAKE3 hash of `content` at retrieval time.
    pub content_hash: String,
}

/// A retrieval result plus a signed log entry proving what ran, when, and
/// against which index. This is the core of point-in-time assurance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestedRetrieval {
    pub documents: Vec<RetrievedDocument>,
    pub attestation: RetrievalAttestation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalAttestation {
    pub query_hash: String,
    pub doc_hashes: Vec<String>,
    pub index: IndexVersion,
    pub ran_at: DateTime<Utc>,
    /// Ed25519 public key of the attesting retrieval service (hex).
    pub signer_pubkey: String,
    /// Ed25519 signature over the canonical serialization of the above (hex).
    pub signature: String,
}

// ── Abstention policy ───────────────────────────────────────────────────────

/// The three outcomes a policy can return for a given query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Decision {
    /// High confidence. Answer directly from parametric memory.
    Answer,
    /// Medium confidence. Answer, but require grounded citations.
    Ground { min_citations: usize },
    /// Low confidence. Refuse to fabricate; return a structured abstention.
    Abstain { reason: AbstainReason },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AbstainReason {
    LongTailEntity { identifier: Option<ExternalId> },
    RetrievalMiss,
    LowCalibratedConfidence { score: f64 },
    ConflictingSources,
}

// ── Provenance ──────────────────────────────────────────────────────────────

/// A completed model response with per-span provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenancedOutput {
    pub text: String,
    pub spans: Vec<ProvenanceSpan>,
    pub decision: Decision,
    pub attestation: Option<RetrievalAttestation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceSpan {
    /// Byte offsets into `ProvenancedOutput::text`, half-open `[start, end)`.
    pub start: usize,
    pub end: usize,
    pub source: ProvenanceSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProvenanceSource {
    /// Backed by a retrieved document.
    Retrieved { doc_id: String },
    /// Computed from a deterministic tool call.
    Computed { tool: String },
    /// Drawn from parametric memory; must be explicitly surfaced to the user.
    Parametric,
}

// ── Traits ──────────────────────────────────────────────────────────────────

#[async_trait]
pub trait EntityResolver: Send + Sync {
    async fn resolve(&self, surface: &str) -> Result<EntityCandidate>;
}

#[async_trait]
pub trait ConfidenceScorer: Send + Sync {
    async fn score(&self, entity: &EntityCandidate) -> Result<Confidence>;
}

#[async_trait]
pub trait RetrievalProvider: Send + Sync {
    async fn retrieve(&self, query: &Query) -> Result<AttestedRetrieval>;
}

pub trait AbstentionPolicy: Send + Sync {
    fn decide(&self, confidence: &Confidence, retrieval: &AttestedRetrieval) -> Decision;
}

/// A grounded prompt is what a [`LlmBackend`] actually sees: the user query
/// plus the attested retrieval context plus the policy decision that governs
/// how the backend must respond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundedPrompt {
    pub user_query: String,
    pub retrieval: AttestedRetrieval,
    pub decision: Decision,
}

#[async_trait]
pub trait LlmBackend: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, prompt: &GroundedPrompt) -> Result<ProvenancedOutput>;
}

impl fmt::Display for PopularityTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Head => "head",
            Self::Torso => "torso",
            Self::LongTail => "long-tail",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    fn init_tracing() {
        let _ =
            tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default());
    }

    #[test]
    fn placeholder() {
        init_tracing();
    }
}
