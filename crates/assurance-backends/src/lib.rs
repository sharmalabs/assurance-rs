//! Pluggable LLM backends.
//!
//! All backends implement [`LlmBackend`] from `assurance-core`. The stub
//! backend is deterministic and useful for tests. Real adapters for Claude
//! (Anthropic Messages API), GPT-class models (OpenAI), and Bedrock wrap
//! their respective HTTP clients and enforce the [`Decision`] contract:
//!
//! * `Decision::Answer`  — call the model with a normal system prompt.
//! * `Decision::Ground`  — require citations in the response, enforced by
//!   structured output and post-validation against the retrieval set.
//! * `Decision::Abstain` — short-circuit, do not call the model at all,
//!   return a structured abstention.

use assurance_core::{
    AbstainReason, Decision, GroundedPrompt, LlmBackend, ProvenanceSource, ProvenanceSpan,
    ProvenancedOutput, Result,
};
use async_trait::async_trait;

/// Deterministic stub backend: honors the policy decision and produces a
/// canned response. Useful for tests and for demonstrating the abstention
/// path without burning API credits.
pub struct StubBackend;

#[async_trait]
impl LlmBackend for StubBackend {
    fn name(&self) -> &str {
        "stub"
    }

    async fn complete(&self, prompt: &GroundedPrompt) -> Result<ProvenancedOutput> {
        match &prompt.decision {
            Decision::Abstain { reason } => {
                let text = abstain_text(reason);
                Ok(ProvenancedOutput {
                    text: text.clone(),
                    spans: vec![ProvenanceSpan {
                        start: 0,
                        end: text.len(),
                        source: ProvenanceSource::Computed {
                            tool: "abstention-policy".into(),
                        },
                    }],
                    decision: prompt.decision.clone(),
                    attestation: Some(prompt.retrieval.attestation.clone()),
                })
            }
            Decision::Ground { min_citations } => {
                let cited: Vec<String> = prompt
                    .retrieval
                    .documents
                    .iter()
                    .take(*min_citations)
                    .map(|d| format!("[{}] {}", d.doc_id, summarize(&d.content)))
                    .collect();
                let text = format!(
                    "Grounded response for: {}\n\nSources:\n{}",
                    prompt.user_query,
                    cited.join("\n")
                );
                let mut spans = Vec::new();
                let mut cursor = 0;
                for d in prompt.retrieval.documents.iter().take(*min_citations) {
                    let frag = format!("[{}]", d.doc_id);
                    if let Some(pos) = text[cursor..].find(&frag) {
                        let start = cursor + pos;
                        let end = start + frag.len();
                        spans.push(ProvenanceSpan {
                            start,
                            end,
                            source: ProvenanceSource::Retrieved {
                                doc_id: d.doc_id.clone(),
                            },
                        });
                        cursor = end;
                    }
                }
                Ok(ProvenancedOutput {
                    text,
                    spans,
                    decision: prompt.decision.clone(),
                    attestation: Some(prompt.retrieval.attestation.clone()),
                })
            }
            Decision::Answer => {
                let text = format!("Direct answer to: {}", prompt.user_query);
                Ok(ProvenancedOutput {
                    text: text.clone(),
                    spans: vec![ProvenanceSpan {
                        start: 0,
                        end: text.len(),
                        source: ProvenanceSource::Parametric,
                    }],
                    decision: prompt.decision.clone(),
                    attestation: Some(prompt.retrieval.attestation.clone()),
                })
            }
        }
    }
}

fn summarize(s: &str) -> String {
    s.chars().take(140).collect::<String>()
}

fn abstain_text(reason: &AbstainReason) -> String {
    match reason {
        AbstainReason::LongTailEntity { .. } => {
            "I don't have reliable information about this specific entity. It appears to be \
             a long-tail business not well-represented in my training data, and retrieval \
             returned no matching documents. Rather than fabricate plausible-sounding details, \
             I'm declining to answer. Try providing the entity's NMLS ID, CIK, or a primary-source \
             document to ground the response."
                .into()
        }
        AbstainReason::RetrievalMiss => {
            "Retrieval returned no documents for this query. I'm declining to answer from \
             parametric memory alone on a factual question."
                .into()
        }
        AbstainReason::LowCalibratedConfidence { score } => {
            format!(
                "Calibrated confidence ({:.2}) is below the grounding threshold. Declining \
                 to answer rather than fabricate.",
                score
            )
        }
        AbstainReason::ConflictingSources => {
            "Retrieved sources conflict on the factual question. Declining to synthesize a \
             single answer without human review."
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assurance_core::{
        AttestedRetrieval, Decision, GroundedPrompt, IndexVersion, RetrievalAttestation,
    };
    use chrono::Utc;

    fn init_tracing() {
        let _ =
            tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default());
    }

    fn abstain_prompt() -> GroundedPrompt {
        GroundedPrompt {
            user_query: "Tell me about Morty".into(),
            retrieval: AttestedRetrieval {
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
            },
            decision: Decision::Abstain {
                reason: assurance_core::AbstainReason::LongTailEntity { identifier: None },
            },
        }
    }

    #[tokio::test]
    async fn stub_abstains_on_long_tail() {
        init_tracing();
        let backend = StubBackend;
        let output = backend.complete(&abstain_prompt()).await.unwrap();
        assert!(matches!(output.decision, Decision::Abstain { .. }));
        assert!(!output.text.is_empty());
    }
}
