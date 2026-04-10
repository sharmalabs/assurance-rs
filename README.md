# assurance-rs

**Point-in-time factuality assurance for long-tail entities in generative AI.**

A Rust workspace demonstrating a working alternative to confident hallucination on small-business and long-tail-entity prompts — the failure mode that disproportionately harms U.S. small businesses and for which no hyperscaler currently offers structural recourse.

---

## What this is

Frontier LLMs fail on long-tail entities (small mortgage brokers, single-state fintechs, regional biotechs, named individuals below the Wikipedia threshold) in a specific, repeatable way: they pattern-match to the largest adjacent concept, produce fluent and confident output, and get the entity-specific facts wrong. The user has no way to tell which parts are real.

This repo is a reference implementation of the stack that would fix it. It is deliberately composed of small, single-purpose crates so that each assurance capability is legible, auditable, and swappable.

Companion research notes:

- *The Long-Tail Factuality Problem — Why Frontier LLMs Fail Small Businesses* (technical)
- *The Recourse Gap — Enterprise Indemnification, SMB Reputational Harm, and the Liability Asymmetry in U.S. Generative AI* (governance)

Both notes diagnose the problem. This repo is the demonstration that it is solvable with engineering, not with a tiered-counterparty conspiracy.

## The four capabilities

| Capability | Crate | What it does |
|---|---|---|
| Entity resolution & confidence | `assurance-entity` | Resolves free-text mentions to NMLS / EDGAR / FDA / state registry identifiers. Absence of a registry match is the strongest long-tail signal. |
| Retrieval with attestation | `assurance-retrieval` | Every retrieval call emits an ed25519-signed log committing to the query, retrieved document hashes, index version, and UTC timestamp. Point-in-time verifiable. |
| Calibrated abstention | `assurance-policy` | Composes confidence + retrieval + popularity tier into one of three outcomes: Answer, Ground, or Abstain. Long-tail + retrieval miss → hard abstention. |
| Provenance tracking | `assurance-core` + `assurance-backends` | Every span of the final output is tagged `Retrieved { doc_id }`, `Computed { tool }`, or `Parametric` — so "this came from a document" and "this came from the model's memory" are visually distinguishable. |

Plus:

- `assurance-eval` — long-tail evaluation harness with samplers for NMLS Consumer Access, EDGAR small filers, FDA establishment registrations, and state secretary-of-state registries. The public benchmark ecosystem (TruthfulQA, FActScore, SimpleQA, HaluEval) under-samples the long tail by construction; this harness samples it directly.
- `assurance-server` — an HTTP / MCP surface exposing the stack to Claude, Cursor, or any MCP-capable client.

## Quick start

### In GitHub Codespaces (one click)

This repo ships a `.devcontainer` config. Open it in Codespaces and wait for `cargo check` to finish on first boot, then:

```bash
cargo run --example morty_mortgage_demo -p assurance-server
```

### Locally

Requires Rust 1.75+ (pin enforced via `rust-toolchain.toml`).

```bash
git clone https://github.com/sharmalabs/assurance-rs
cd assurance-rs
cargo test --workspace
cargo run --example morty_mortgage_demo -p assurance-server
```

Expected output:

```
=== Morty Mortgage demo ===
Entity tier:    unknown
Confidence:     0.035
Decision:       Abstain { reason: LongTailEntity { identifier: None } }

--- Output ---
I don't have reliable information about this specific entity. It appears to be
a long-tail business not well-represented in my training data, and retrieval
returned no matching documents. Rather than fabricate plausible-sounding
details, I'm declining to answer. Try providing the entity's NMLS ID, CIK, or
a primary-source document to ground the response.
```

Compare this to what a frontier LLM produces for the same prompt without the assurance stack. That difference is the point.

## Status

This is a reference architecture, not a production service. Trait definitions and wiring are complete; several components ship stubs:

- `assurance-backends` includes a deterministic `StubBackend`. Live adapters for Anthropic Messages API, OpenAI, and Bedrock are the next step.
- `assurance-eval` ships sampler traits with empty implementations. The registries are all publicly accessible under U.S. federal and state open-data regimes; wiring them is a matter of HTTP clients and rate-limit discipline, not architecture.
- `assurance-server` ships a CLI entry point. The MCP tool surface is described in `DESIGN.md` § 5 and is the next PR.

See `DESIGN.md` for the architectural rationale, type contracts, attestation format, and evaluation methodology.

## License

Apache-2.0. See `LICENSE`.

## Author

Shantanu ("Shanta") Sharma, PhD — Founder & CEO, Sharma Labs, Inc. PhD Biochemistry (UNC Chapel Hill), B.Tech Computer Science (IIT Kanpur). Peer reviewer, NeurIPS / ICML / CLeaR.

Research feedback and PRs welcome.
