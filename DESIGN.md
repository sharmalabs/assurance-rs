# assurance-rs — Design Document

**Author:** Shantanu Sharma, PhD (Sharma Labs, Inc.)
**Status:** Draft v0.1 — reference architecture
**Target runtime:** Rust 1.75+, Tokio

---

## 1. Motivation

The companion research notes argue two things:

1. The "tiered enterprise counterparty list" theory of hyperscaler hallucination is not supported by any documented architecture. The real failure mode is long-tail data sparsity plus sycophantic completion under RLHF plus missing retrieval grounding plus benchmarks that under-sample the long tail.
2. The enterprise-vs-SMB recourse gap is real. Every hyperscaler indemnity covers IP claims against the enterprise customer; none cover defamation or reputational harm inflicted on a third-party small-business subject of a generated output.

This document specifies the technical layer that would address the first problem directly and would give the second problem something concrete to regulate. The whole stack is built around one principle: **never confabulate on a long-tail entity; either ground the response in attested retrieval or return a structured abstention.**

## 2. Design goals

- **Runtime-agnostic core.** The `assurance-core` crate depends only on `serde`, `thiserror`, `async-trait`, and `chrono`. No Tokio, no HTTP, no global state. Every other crate is an implementation of a core trait.
- **Point-in-time verifiability.** An auditor with the retrieval service's public key and a persisted attestation must be able to reconstruct exactly what the model saw and when, without access to the running service.
- **Explicit abstention as a first-class outcome.** Abstention is not an error and not a fallback. It is one of three policy outcomes (`Answer`, `Ground`, `Abstain`) that every call must return.
- **Public-registry-grounded evaluation.** The long-tail eval harness samples entities from U.S. public registries (NMLS, EDGAR, FDA, state SOS), not from Wikipedia. If a benchmark's ceiling is Wikipedia, it cannot measure the long-tail failure mode by construction.
- **Swappable backends under a single contract.** Any LLM provider plugs in by implementing `LlmBackend`, and the contract requires the backend to honor the policy decision — grounding when asked to ground, abstaining when asked to abstain, never calling the model on an abstention path.

## 3. Crate layout

```
assurance-rs/
├── Cargo.toml                       workspace root
├── rust-toolchain.toml              pinned 1.75
├── .devcontainer/                   Codespaces config
├── .github/workflows/ci.yml         fmt + test
└── crates/
    ├── assurance-core/              traits, types, errors
    ├── assurance-entity/            registry resolver + confidence scorer
    ├── assurance-retrieval/         retriever + ed25519 attestation
    ├── assurance-policy/            calibrated abstention policy
    ├── assurance-eval/              long-tail eval harness + samplers
    ├── assurance-backends/          LlmBackend adapters (stub + live)
    └── assurance-server/            HTTP + MCP surface, binary, examples
```

Dependency direction is strictly downstream: `core` is depended on by every other crate; nothing depends on `server` or `eval`.

## 4. Type contracts

### 4.1 Entity identity

```rust
pub enum ExternalId {
    Nmls(String),                               // NMLS Consumer Access
    Cik(String),                                // SEC EDGAR Central Index Key
    FdaEstablishment(String),                   // FDA establishment registration
    StateBusiness { state: String, id: String },
    Other { scheme: String, id: String },
}
```

Registry IDs are the anchor. "Vema Mortgage" is ambiguous; `ExternalId::Nmls("1234567")` is not. Absence of any registry match for a mention is the strongest single signal that the entity is in the long-tail failure region.

### 4.2 Confidence

`Confidence { score: f64, signals: Vec<ConfidenceSignal> }`. The reference scorer combines a popularity-tier signal and a registry-match signal. Production scorers can compose additional signals: web retrieval hit count, citation density, training-corpus frequency estimates via logit lens or perplexity thresholding.

The score is calibrated, not raw — `score = Σ weight_i · value_i` clamped to `[0, 1]`. Weights sum to 1.

### 4.3 Retrieval attestation

Every retrieval call produces a `RetrievalAttestation`:

```rust
pub struct RetrievalAttestation {
    pub query_hash: String,         // BLAKE3(query)
    pub doc_hashes: Vec<String>,    // BLAKE3(content) per retrieved doc
    pub index: IndexVersion,        // { name, version, built_at }
    pub ran_at: DateTime<Utc>,
    pub signer_pubkey: String,      // ed25519 verifying key, hex
    pub signature: String,          // ed25519 signature, hex
}
```

The signature commits to the canonical payload:

```
assurance-rs/v1
query:<hex>
index:<name>@<version>
built_at:<rfc3339>
ran_at:<rfc3339>
docs:<hex>,<hex>,...
```

This is a v1 format. Production deployments should migrate to a formal canonical encoding (CBOR via `dag-cbor`, or RFC 8785 JCS) before the first externally-visible release.

The signing key is held by the retrieval service. In production it lives in a KMS/HSM; in the reference implementation it is generated at startup by `OsRng`. `verify_attestation()` in `assurance-retrieval` is the reference verifier — an auditor, a defamation litigant under a hypothetical notice-and-correction regime, or a regulator can call it with only the public key and the persisted attestation blob.

**What this gives you:** if a small business claims "the model fabricated this about us on April 10, 2026," the hyperscaler can either produce the attested retrieval log showing what the model actually saw, or it cannot. Either answer is materially better than the current state where neither party has any reconstructable record.

### 4.4 Decision

```rust
pub enum Decision {
    Answer,                                      // high confidence, parametric
    Ground { min_citations: usize },             // medium, require citations
    Abstain { reason: AbstainReason },           // low, refuse to fabricate
}
```

The `ThresholdPolicy` in `assurance-policy` is the reference implementation. Defaults:

- `answer_threshold: 0.85`
- `ground_threshold: 0.50`
- `min_citations: 2`
- `require_retrieval_for_long_tail: true`

The long-tail rule is a hard short-circuit: if `tier ∈ {LongTail, Unknown}` and retrieval returned zero documents, the policy returns `Abstain { LongTailEntity }` regardless of the confidence score. This is the policy analog of "when in doubt, don't."

### 4.5 Provenance

```rust
pub struct ProvenancedOutput {
    pub text: String,
    pub spans: Vec<ProvenanceSpan>,
    pub decision: Decision,
    pub attestation: Option<RetrievalAttestation>,
}

pub enum ProvenanceSource {
    Retrieved { doc_id: String },
    Computed  { tool: String },
    Parametric,
}
```

Every span of the output text is tagged with its source. A UI rendering a `ProvenancedOutput` can visually distinguish retrieved-from-doc content (trustworthy given the attestation) from parametric-memory content (must be verified by the user). The abstention path returns a single `Computed { tool: "abstention-policy" }` span — the refusal itself is a computation, not a fabrication.

## 5. MCP tool surface (planned)

`assurance-server` will expose the stack as an MCP server so that any MCP-capable client (Claude, Cursor, Continue, Zed) can call the assurance layer as tools:

| Tool | Input | Output |
|---|---|---|
| `assurance.resolve` | `{ surface: string }` | `EntityCandidate` |
| `assurance.score` | `EntityCandidate` | `Confidence` |
| `assurance.retrieve` | `{ query, entity?, index? }` | `AttestedRetrieval` |
| `assurance.decide` | `{ confidence, retrieval, tier }` | `Decision` |
| `assurance.complete` | `GroundedPrompt` | `ProvenancedOutput` |
| `assurance.verify` | `{ pubkey, attestation }` | `{ valid: bool }` |

This lets Claude (or any other model) call the assurance layer as a tool during normal generation, so the model can introspect its own confidence on a named entity before committing to an answer.

## 6. Evaluation

Public benchmarks do not measure the failure mode this stack addresses, because they under-sample the long tail.

`assurance-eval` defines:

- `Sampler` trait — samples `EvalCase { entity, prompt, atomic_facts }` from a registry.
- Four reference samplers: `NmlsSampler`, `EdgarSmallFilerSampler`, `FdaEstablishmentSampler`, `StateBusinessSampler`.
- `run_suite(&backend, cases)` — runs each case end-to-end, decomposes outputs into atomic facts via an LLM-judge or FActScore-style pipeline, and scores against the ground-truth list from the registry.

Per-case scoring returns `{ facts_correct, facts_wrong, facts_unsupported, abstained }`. A backend that abstains on a case where it lacked evidence receives credit proportional to the abstention rate of the evaluator's expected-answerable subset — abstention is rewarded, confabulation is punished, and both are measured separately from grounded-citation accuracy.

The intended output is a stratified scorecard with four columns (Head, Torso, LongTail, Unknown) rather than a single scalar. A model that scores 0.95 on Head and 0.20 on LongTail is a different product than one that scores 0.75 across the board, and buyers deserve to see both.

## 7. What is deliberately not in scope

- **Fine-tuning.** This architecture does not fine-tune any model. Fine-tuning improves format and style on customer data; it does not improve factuality about third-party entities outside the fine-tune set, and it can increase confident-error rates.
- **Guardrails for harmful content.** `assurance-rs` addresses factual accuracy. PII filtering, CSAM detection, jailbreak resistance, and content moderation are handled by orthogonal systems.
- **Vector retrieval.** The reference `AttestingRetriever` uses a naive substring match for the demo. Production deployments wire attestation around Elasticsearch, Vespa, Vertex AI Search, or pgvector. The attestation format is retrieval-engine-agnostic by design.

## 8. Open questions

- **Canonical encoding for the attestation payload.** v1 uses a simple newline-delimited format. Before any externally-consumed release this should move to RFC 8785 JCS or dag-cbor.
- **Confidence calibration.** The reference scorer produces a weighted sum that is not formally calibrated. Production scorers need an isotonic regression or Platt scaling stage against held-out eval data.
- **Revocation.** If an attestation key is compromised, what is the revocation path and how are historical attestations re-verified? The current design has no answer; a transparency-log model (à la Certificate Transparency) is the most likely direction.
- **Composition with tool use.** A model that calls tools mid-generation produces output that is partially retrieved, partially computed, and partially parametric. The `ProvenanceSpan` type supports this, but the backend adapter layer currently does not track tool calls as distinct provenance sources.

## 9. Why Rust

Three reasons, in order of importance:

1. **Latency budget.** The assurance layer sits inline between user request and model response. In financial services (low-latency equity execution, credit decisioning, KYB) the layer cannot add more than low-single-digit milliseconds. Rust's zero-cost abstractions and predictable GC-free latency distribution make this achievable; an equivalent Python layer adds 50–200 ms of tail latency before any real work happens.
2. **Memory safety without GC pauses.** Attestation logs and retrieval results are hot-path structures handled under load. Rust's borrow checker eliminates the class of bugs (use-after-free, data races) that would otherwise produce attestation corruption under concurrency — which would be catastrophic for the audit use case.
3. **Ecosystem fit.** `tokio`, `axum`, `serde`, `ed25519-dalek`, and `blake3` are mature, widely audited, and zero-cost. The MCP Rust SDK exists and is actively maintained. The cost of Rust for this workload is low and the benefit is structural.

---

*This document will evolve. Feedback, especially from practitioners in U.S. financial services AI and life sciences AI, is welcome via GitHub Issues.*
