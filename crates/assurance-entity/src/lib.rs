//! Entity resolution and confidence scoring.
//!
//! The [`RegistryResolver`] attempts to map a free-text mention to a stable
//! registry identifier (NMLS, CIK, FDA establishment, state business ID).
//! Resolution confidence is the single strongest signal for downstream
//! calibrated abstention — if no registry match exists, the entity is almost
//! certainly in the long-tail failure region and the model should not
//! confabulate.
//!
//! This crate ships trait definitions and an in-memory reference implementation
//! keyed off a fixture table. Production deployments wire the trait to the
//! actual public registries — all of which expose bulk downloads or APIs under
//! U.S. federal or state open-data regimes.

use assurance_core::{
    AssuranceError, Confidence, ConfidenceScorer, ConfidenceSignal, EntityCandidate,
    EntityResolver, ExternalId, PopularityTier, Result,
};
use async_trait::async_trait;
use std::collections::HashMap;

/// In-memory registry resolver backed by a pre-loaded fixture table.
///
/// This is the reference implementation. Replace with live NMLS / EDGAR / FDA
/// clients in production.
pub struct RegistryResolver {
    table: HashMap<String, Vec<ExternalId>>,
    tiers: HashMap<String, PopularityTier>,
}

impl RegistryResolver {
    pub fn new() -> Self {
        Self {
            table: HashMap::new(),
            tiers: HashMap::new(),
        }
    }

    /// Load a fixture from a JSON file mapping surface forms to external IDs.
    ///
    /// See `crates/assurance-entity/fixtures/` for examples.
    pub fn load_fixture(&mut self, json: &str) -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Row {
            surface: String,
            ids: Vec<ExternalId>,
            #[serde(default)]
            tier: Option<String>,
        }
        let rows: Vec<Row> = serde_json::from_str(json)?;
        for r in rows {
            let key = normalize(&r.surface);
            self.table.insert(key.clone(), r.ids);
            if let Some(t) = r.tier {
                self.tiers.insert(key, parse_tier(&t));
            }
        }
        Ok(())
    }
}

impl Default for RegistryResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EntityResolver for RegistryResolver {
    async fn resolve(&self, surface: &str) -> Result<EntityCandidate> {
        let key = normalize(surface);
        let ids = self.table.get(&key).cloned().unwrap_or_default();
        let tier = self.tiers.get(&key).copied().unwrap_or(if ids.is_empty() {
            PopularityTier::Unknown
        } else {
            PopularityTier::LongTail
        });
        Ok(EntityCandidate {
            surface: surface.to_string(),
            identifiers: ids,
            tier,
        })
    }
}

/// Reference confidence scorer.
///
/// This implementation is deliberately conservative: it treats absence of a
/// registry match as a near-zero confidence prior, which is the whole point.
/// Production scorers can compose additional signals (web retrieval hits,
/// citation density, training-corpus frequency estimates).
pub struct RegistryConfidenceScorer;

#[async_trait]
impl ConfidenceScorer for RegistryConfidenceScorer {
    async fn score(&self, entity: &EntityCandidate) -> Result<Confidence> {
        let tier_signal = match entity.tier {
            PopularityTier::Head => 0.95,
            PopularityTier::Torso => 0.70,
            PopularityTier::LongTail => 0.25,
            PopularityTier::Unknown => 0.05,
        };
        let id_signal = if entity.identifiers.is_empty() {
            0.0
        } else {
            0.4
        };

        let signals = vec![
            ConfidenceSignal {
                name: "popularity_tier".into(),
                weight: 0.7,
                value: tier_signal,
            },
            ConfidenceSignal {
                name: "registry_id_present".into(),
                weight: 0.3,
                value: id_signal,
            },
        ];
        let score: f64 = signals
            .iter()
            .map(|s| s.weight * s.value)
            .sum::<f64>()
            .min(1.0);

        Ok(Confidence { score, signals })
    }
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

fn parse_tier(s: &str) -> PopularityTier {
    match s {
        "head" => PopularityTier::Head,
        "torso" => PopularityTier::Torso,
        "long-tail" | "longtail" => PopularityTier::LongTail,
        _ => PopularityTier::Unknown,
    }
}

/// Convenience error mapper so callers can use `?` with the core error type.
pub fn wrap<E: std::fmt::Display>(e: E) -> AssuranceError {
    AssuranceError::Resolution(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_tracing() {
        let _ =
            tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default());
    }

    #[tokio::test]
    async fn unknown_entity_scores_low() {
        init_tracing();
        let resolver = RegistryResolver::new();
        let candidate = resolver.resolve("nonexistent co").await.unwrap();
        let scorer = RegistryConfidenceScorer;
        let confidence = scorer.score(&candidate).await.unwrap();
        assert!(confidence.score < 0.1);
    }
}
