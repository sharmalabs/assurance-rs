//! End-to-end example: the Morty Mortgage failure case.
//!
//! Run with:
//!     cargo run --example morty_mortgage_demo -p assurance-server
//!
//! Demonstrates that the assurance stack either abstains (if the entity
//! has no registry match and no retrieval hit) or grounds the response
//! against attested retrieved documents — never confabulates.

use assurance_backends::StubBackend;
use assurance_core::{
    ConfidenceScorer, EntityResolver, GroundedPrompt, IndexVersion, LlmBackend, Query,
    RetrievalProvider,
};
use assurance_entity::{RegistryConfidenceScorer, RegistryResolver};
use assurance_policy::ThresholdPolicy;
use assurance_retrieval::AttestingRetriever;
use chrono::Utc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let resolver = RegistryResolver::new(); // empty — simulates long-tail miss
    let scorer = RegistryConfidenceScorer;

    let index = IndexVersion {
        name: "empty-corpus".into(),
        version: "0".into(),
        built_at: Utc::now(),
    };
    let retriever = AttestingRetriever::new(index.clone());
    let policy = ThresholdPolicy::default();
    let backend = StubBackend;

    let user_query = "Write a business growth document for leadership at Morty, \
         focused on NY mortgage originations.";

    let entity = resolver.resolve("Morty").await?;
    let confidence = scorer.score(&entity).await?;
    let query = Query {
        text: user_query.into(),
        entity: Some(entity.clone()),
        index,
    };
    let retrieval = retriever.retrieve(&query).await?;
    let decision = policy.decide_with_tier(&confidence, &retrieval, entity.tier);

    let prompt = GroundedPrompt {
        user_query: user_query.into(),
        retrieval,
        decision,
    };
    let output = backend.complete(&prompt).await?;

    println!("=== Morty demo ===");
    println!("Entity tier:    {}", entity.tier);
    println!("Confidence:     {:.3}", confidence.score);
    println!("Decision:       {:?}", output.decision);
    println!("\n--- Output ---\n{}\n", output.text);
    Ok(())
}
