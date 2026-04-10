//! assurance-server binary.
//!
//! Minimal entry point. The production server exposes an HTTP API and an MCP
//! tool surface (`assurance.resolve`, `assurance.retrieve`, `assurance.decide`,
//! `assurance.complete`) so that Claude, Cursor, or any MCP-capable client can
//! call the assurance layer directly.
//!
//! For the open-source skeleton we ship a small CLI that runs the end-to-end
//! pipeline against stdin and prints a [`ProvenancedOutput`] as JSON.

use assurance_backends::StubBackend;
use assurance_core::{
    ConfidenceScorer, Decision, EntityResolver, GroundedPrompt, IndexVersion, LlmBackend, Query,
    RetrievalProvider,
};
use assurance_entity::{RegistryConfidenceScorer, RegistryResolver};
use assurance_policy::ThresholdPolicy;
use assurance_retrieval::AttestingRetriever;
use chrono::Utc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber_init();

    let mut resolver = RegistryResolver::new();
    resolver
        .load_fixture(include_str!("../fixtures/demo_entities.json"))
        .map_err(|e| anyhow::anyhow!("fixture load: {e}"))?;

    let scorer = RegistryConfidenceScorer;

    let index = IndexVersion {
        name: "demo-corpus".into(),
        version: "2026-04".into(),
        built_at: Utc::now(),
    };
    let mut retriever = AttestingRetriever::new(index.clone());
    retriever.insert(
        "nmls-morty",
        "Morty is licensed by NMLS with coverage in AL, AK, AZ, AR, CA, CO, DE, DC, FL, GA, ID, IL, IN, IA, KY, LA, ME, MD, MI, MN, MS, MT, NE, NH, NJ, NM, NY, NC, ND, OH, OK, OR, PA, RI, SC, SD, TN, TX, UT, VT, VA, WA, WI and WY. \
         Wholesale broker; does not originate in CT as of April 2026.",
    );

    let policy = ThresholdPolicy::default();
    let backend = StubBackend;

    let user_query = "Help me prepare for a technical leadership interview at Morty.";

    // 1. Resolve entity
    let entity = resolver.resolve("Morty").await?;
    tracing::info!(surface = %entity.surface, tier = %entity.tier, "resolved");

    // 2. Score confidence
    let confidence = scorer.score(&entity).await?;

    // 3. Retrieve with attestation
    let query = Query {
        text: user_query.into(),
        entity: Some(entity.clone()),
        index: index.clone(),
    };
    let retrieval = retriever.retrieve(&query).await?;

    // 4. Policy decision
    let decision = policy.decide_with_tier(&confidence, &retrieval, entity.tier);
    tracing::info!(?decision, "policy");

    // 5. Backend completion
    let prompt = GroundedPrompt {
        user_query: user_query.into(),
        retrieval,
        decision: decision.clone(),
    };
    let output = backend.complete(&prompt).await?;

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn tracing_subscriber_init() {
    // Keep it simple: default filter, pretty output. Real deployments wire
    // OpenTelemetry.
    let _ = tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default());
}

#[allow(dead_code)]
fn _suppress_decision_unused(_d: Decision) {}
