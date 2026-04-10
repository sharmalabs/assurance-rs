//! Calibrated abstention and provenance policy.
//!
//! A [`ThresholdPolicy`] composes three inputs — a confidence score, a
//! retrieval result, and a popularity tier — and returns one of three
//! outcomes:
//!
//! * [`Decision::Answer`] — high confidence, answer directly.
//! * [`Decision::Ground`] — medium confidence, require grounded citations.
//! * [`Decision::Abstain`] — low confidence, refuse to fabricate.
//!
//! The default thresholds are deliberately conservative. They are the
//! policy analog of "when in doubt, don't" — the opposite of sycophantic
//! completion under RLHF.

use assurance_core::{
    AbstainReason, AbstentionPolicy, AttestedRetrieval, Confidence, Decision, PopularityTier,
};

#[derive(Debug, Clone)]
pub struct ThresholdPolicy {
    pub answer_threshold: f64,
    pub ground_threshold: f64,
    pub min_citations: usize,
    pub require_retrieval_for_long_tail: bool,
}

impl Default for ThresholdPolicy {
    fn default() -> Self {
        Self {
            answer_threshold: 0.85,
            ground_threshold: 0.50,
            min_citations: 2,
            require_retrieval_for_long_tail: true,
        }
    }
}

impl ThresholdPolicy {
    pub fn decide_with_tier(
        &self,
        confidence: &Confidence,
        retrieval: &AttestedRetrieval,
        tier: PopularityTier,
    ) -> Decision {
        // Hard rule: long-tail entity with zero retrieval hits always abstains.
        if self.require_retrieval_for_long_tail
            && matches!(tier, PopularityTier::LongTail | PopularityTier::Unknown)
            && retrieval.documents.is_empty()
        {
            return Decision::Abstain {
                reason: AbstainReason::LongTailEntity { identifier: None },
            };
        }

        if confidence.score >= self.answer_threshold && !retrieval.documents.is_empty() {
            Decision::Answer
        } else if confidence.score >= self.ground_threshold {
            Decision::Ground {
                min_citations: self.min_citations,
            }
        } else {
            Decision::Abstain {
                reason: AbstainReason::LowCalibratedConfidence {
                    score: confidence.score,
                },
            }
        }
    }
}

impl AbstentionPolicy for ThresholdPolicy {
    fn decide(&self, confidence: &Confidence, retrieval: &AttestedRetrieval) -> Decision {
        // When tier is not provided at call site, fall back to Unknown which
        // forces retrieval requirement.
        self.decide_with_tier(confidence, retrieval, PopularityTier::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assurance_core::{
        AttestedRetrieval, Confidence, ConfidenceSignal, IndexVersion, PopularityTier,
        RetrievalAttestation,
    };
    use chrono::Utc;

    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
    }

    fn empty_retrieval() -> AttestedRetrieval {
        AttestedRetrieval {
            documents: vec![],
            attestation: RetrievalAttestation {
                query_hash: String::new(),
                doc_hashes: vec![],
                index: IndexVersion {
                    name: "test".into(),
                    version: "1".into(),
                    built_at: Utc::now(),
                },
                ran_at: Utc::now(),
                signer_pubkey: String::new(),
                signature: String::new(),
            },
        }
    }

    fn low_confidence() -> Confidence {
        Confidence {
            score: 0.05,
            signals: vec![ConfidenceSignal {
                name: "tier".into(),
                weight: 1.0,
                value: 0.05,
            }],
        }
    }

    #[test]
    fn long_tail_no_retrieval_abstains() {
        init_tracing();
        let policy = ThresholdPolicy::default();
        let decision = policy.decide_with_tier(
            &low_confidence(),
            &empty_retrieval(),
            PopularityTier::Unknown,
        );
        assert!(matches!(decision, Decision::Abstain { .. }));
    }
}
