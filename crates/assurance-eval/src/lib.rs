//! Long-tail evaluation harness.
//!
//! Public hallucination benchmarks (TruthfulQA, FActScore, SimpleQA,
//! HaluEval) are dominated by general-knowledge or Wikipedia-ceiling
//! prompts. This harness is the missing piece: it samples entities from
//! U.S. public registries and scores a backend's factual accuracy on
//! the long tail specifically.
//!
//! Samplers (stubs here, live implementations in downstream forks):
//!
//! * [`NmlsSampler`] — NMLS Consumer Access (mortgage loan originators).
//! * [`EdgarSmallFilerSampler`] — SEC EDGAR small-filer 10-K section.
//! * [`FdaEstablishmentSampler`] — FDA establishment registration.
//! * [`StateBusinessSampler`] — Secretary of State business entity registries.

use assurance_core::{ExternalId, LlmBackend, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub entity: ExternalId,
    pub prompt: String,
    /// Ground-truth atomic facts drawn from the registry record.
    pub atomic_facts: Vec<AtomicFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicFact {
    pub predicate: String,
    pub value: String,
    /// URL of the primary-source document the fact was extracted from.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub case: EvalCase,
    pub backend: String,
    pub facts_correct: usize,
    pub facts_wrong: usize,
    pub facts_unsupported: usize,
    pub abstained: bool,
}

#[async_trait]
pub trait Sampler: Send + Sync {
    async fn sample(&self, n: usize) -> Result<Vec<EvalCase>>;
}

/// NMLS Consumer Access sampler. Live implementation would fetch from
/// `https://www.nmlsconsumeraccess.org/` bulk downloads.
pub struct NmlsSampler;
#[async_trait]
impl Sampler for NmlsSampler {
    async fn sample(&self, _n: usize) -> Result<Vec<EvalCase>> {
        Ok(vec![])
    }
}

pub struct EdgarSmallFilerSampler;
#[async_trait]
impl Sampler for EdgarSmallFilerSampler {
    async fn sample(&self, _n: usize) -> Result<Vec<EvalCase>> {
        Ok(vec![])
    }
}

pub struct FdaEstablishmentSampler;
#[async_trait]
impl Sampler for FdaEstablishmentSampler {
    async fn sample(&self, _n: usize) -> Result<Vec<EvalCase>> {
        Ok(vec![])
    }
}

pub struct StateBusinessSampler {
    pub state: String,
}
#[async_trait]
impl Sampler for StateBusinessSampler {
    async fn sample(&self, _n: usize) -> Result<Vec<EvalCase>> {
        Ok(vec![])
    }
}

/// Run a suite of eval cases against a backend, producing per-case atomic-fact
/// accuracy. See DESIGN.md § "Evaluation" for the scoring semantics.
pub async fn run_suite<B: LlmBackend + ?Sized>(
    _backend: &B,
    _cases: Vec<EvalCase>,
) -> Result<Vec<EvalResult>> {
    // Wiring: for each case, construct a GroundedPrompt, call backend.complete,
    // decompose the output into atomic facts (e.g. via an LLM judge or
    // FActScore-style pipeline), and compare against the ground-truth list.
    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
    }

    #[tokio::test]
    async fn samplers_return_empty_stubs() {
        init_tracing();
        assert!(NmlsSampler.sample(5).await.unwrap().is_empty());
        assert!(EdgarSmallFilerSampler.sample(5).await.unwrap().is_empty());
        assert!(FdaEstablishmentSampler.sample(5).await.unwrap().is_empty());
        assert!(StateBusinessSampler { state: "NC".into() }
            .sample(5)
            .await
            .unwrap()
            .is_empty());
    }
}
