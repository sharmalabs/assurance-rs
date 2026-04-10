//! Retrieval with point-in-time cryptographic attestation.
//!
//! Every retrieval call produces a [`RetrievalAttestation`] that commits to:
//!
//! * the BLAKE3 hash of the canonical query,
//! * the BLAKE3 hash of every returned document,
//! * the index name and version at the moment of retrieval,
//! * a UTC timestamp,
//!
//! signed with an ed25519 key held by the retrieval service. Downstream
//! consumers (auditors, defamation litigants, regulators under a
//! hypothetical notice-and-correction regime) can later verify exactly
//! which documents the model saw and when.

use assurance_core::{
    AssuranceError, AttestedRetrieval, IndexVersion, Query, Result, RetrievalAttestation,
    RetrievalProvider, RetrievedDocument,
};
use async_trait::async_trait;
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;

/// In-memory retriever with an ed25519 attestation key.
///
/// Reference implementation — production deployments wire this to Elasticsearch,
/// Vespa, Vertex AI Search, or any vector store, and persist the signing key in
/// a KMS/HSM.
pub struct AttestingRetriever {
    corpus: Vec<RetrievedDocument>,
    index: IndexVersion,
    signing_key: SigningKey,
}

impl AttestingRetriever {
    pub fn new(index: IndexVersion) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self {
            corpus: Vec::new(),
            index,
            signing_key,
        }
    }

    pub fn with_key(index: IndexVersion, signing_key: SigningKey) -> Self {
        Self {
            corpus: Vec::new(),
            index,
            signing_key,
        }
    }

    pub fn insert(&mut self, doc_id: impl Into<String>, content: impl Into<String>) {
        let content = content.into();
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        self.corpus.push(RetrievedDocument {
            doc_id: doc_id.into(),
            source_url: None,
            content,
            content_hash,
        });
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    fn sign(&self, query_hash: &str, doc_hashes: &[String]) -> Result<RetrievalAttestation> {
        let ran_at = Utc::now();
        let index = self.index.clone();
        let canonical = canonical_attestation_payload(query_hash, doc_hashes, &index, &ran_at);
        let signature = self.signing_key.sign(canonical.as_bytes());

        Ok(RetrievalAttestation {
            query_hash: query_hash.to_string(),
            doc_hashes: doc_hashes.to_vec(),
            index,
            ran_at,
            signer_pubkey: hex::encode(self.verifying_key().to_bytes()),
            signature: hex::encode(signature.to_bytes()),
        })
    }
}

#[async_trait]
impl RetrievalProvider for AttestingRetriever {
    async fn retrieve(&self, query: &Query) -> Result<AttestedRetrieval> {
        // Reference implementation: naive substring match. Swap for real
        // semantic retrieval in production.
        let needle = query.text.to_lowercase();
        let docs: Vec<RetrievedDocument> = self
            .corpus
            .iter()
            .filter(|d| d.content.to_lowercase().contains(&needle))
            .cloned()
            .collect();

        let query_hash = blake3::hash(query.text.as_bytes()).to_hex().to_string();
        let doc_hashes: Vec<String> = docs.iter().map(|d| d.content_hash.clone()).collect();
        let attestation = self.sign(&query_hash, &doc_hashes)?;

        Ok(AttestedRetrieval {
            documents: docs,
            attestation,
        })
    }
}

/// Canonical byte representation committed to by the signature. Kept simple
/// and deterministic; production implementations should use a formal canonical
/// JSON or CBOR encoder.
pub fn canonical_attestation_payload(
    query_hash: &str,
    doc_hashes: &[String],
    index: &IndexVersion,
    ran_at: &chrono::DateTime<Utc>,
) -> String {
    format!(
        "assurance-rs/v1\nquery:{}\nindex:{}@{}\nbuilt_at:{}\nran_at:{}\ndocs:{}",
        query_hash,
        index.name,
        index.version,
        index.built_at.to_rfc3339(),
        ran_at.to_rfc3339(),
        doc_hashes.join(","),
    )
}

/// Verify a retrieval attestation given the verifying key. This is what an
/// auditor or defamation litigant would call on a persisted attestation to
/// prove (or disprove) that a specific corpus state produced a specific
/// output at a specific time.
pub fn verify_attestation(pubkey_hex: &str, attestation: &RetrievalAttestation) -> Result<bool> {
    use ed25519_dalek::Verifier;
    let pk_bytes = hex::decode(pubkey_hex)
        .map_err(|e| AssuranceError::Attestation(format!("pubkey hex: {e}")))?;
    let pk_arr: [u8; 32] = pk_bytes
        .try_into()
        .map_err(|_| AssuranceError::Attestation("pubkey length".into()))?;
    let verifying_key = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|e| AssuranceError::Attestation(format!("pubkey parse: {e}")))?;

    let sig_bytes = hex::decode(&attestation.signature)
        .map_err(|e| AssuranceError::Attestation(format!("sig hex: {e}")))?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| AssuranceError::Attestation("sig length".into()))?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

    let canonical = canonical_attestation_payload(
        &attestation.query_hash,
        &attestation.doc_hashes,
        &attestation.index,
        &attestation.ran_at,
    );
    Ok(verifying_key
        .verify(canonical.as_bytes(), &signature)
        .is_ok())
}

// Small hex helper so we don't pull the `hex` crate for four calls.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }
    pub fn decode(s: &str) -> std::result::Result<Vec<u8>, String> {
        if s.len() % 2 != 0 {
            return Err("odd length".into());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assurance_core::IndexVersion;

    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
    }

    fn idx() -> IndexVersion {
        IndexVersion {
            name: "test".into(),
            version: "1".into(),
            built_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn attestation_roundtrip_verifies() {
        init_tracing();
        let mut r = AttestingRetriever::new(idx());
        r.insert(
            "doc1",
            "Morty is an NMLS-registered wholesale broker",
        );
        let q = Query {
            text: "morty".into(),
            entity: None,
            index: r.index.clone(),
        };
        let res = <AttestingRetriever as RetrievalProvider>::retrieve(&r, &q)
            .await
            .unwrap();
        let pk = hex::encode(r.verifying_key().to_bytes());
        assert!(verify_attestation(&pk, &res.attestation).unwrap());
    }
}
