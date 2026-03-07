# ADR-011: Embedding Providers Are Adapters

**Status:** Accepted
**Date:** 2026-03-07

## Context

Ollama is a popular tool for running LLM and embedding models locally. Its rise has made local-first embedding operationally attractive, and shiro targets a local-first use case. However, Ollama is not the only viable embedding provider: OpenAI, compatible self-hosted endpoints (e.g., llama.cpp server, vLLM, Mistral), and future runtimes are all legitimate deployment targets.

Architecturally coupling shiro to Ollama would mean:
- Assuming a specific runtime is installed on the user's machine
- Assuming local inference is the canonical deployment mode
- Encoding provider-specific behavior into retrieval and indexing logic

`HttpEmbedder` in `shiro-embed` already targets any OpenAI-compatible `/v1/embeddings` endpoint. Ollama exposes this same interface. This means Ollama support is already present through the generic adapter — no special-casing is required.

The `Embedder` trait in `shiro-core::ports` defines the correct abstraction boundary: `embed()`, `embed_batch()`, and `dimensions()`. Any implementation behind that boundary is a swappable adapter.

## Decision

- shiro must not depend on Ollama as the canonical or required embedding architecture.
- shiro supports Ollama-compatible endpoints as one concrete `Embedder` adapter via `HttpEmbedder` in `shiro-embed`, which targets any OpenAI-compatible `/v1/embeddings` endpoint. No Ollama-specific SDK dependency is introduced.
- The canonical embedding boundary is the `Embedder` trait in `shiro-core::ports`. All retrieval and indexing logic (`shiro-sdk` fusion via RRF, `shiro-index` generation tracking, `shiro-embed::FlatIndex`) must program against this trait exclusively.
- All embedding outputs must carry provider, model, and fingerprint metadata (via `ProcessingFingerprint { parser_name, parser_version, segmenter_version }` extended with embedder identity) to enable correctness checks across provider changes.
- Embedding identity must not be inferred from model name alone. Dimension count and versioned fingerprint together constitute identity.

### Deliberate Absences

- No direct dependency from retrieval core (`shiro-sdk`, `shiro-index`) to Ollama or any specific provider.
- No assumption that Ollama is installed on the host system.
- No assumption that local inference is the only deployment mode.
- No embedding identity inferred from model name alone — dimensions and versioned fingerprint constitute identity.

## Consequences

- Any OpenAI-compatible embedding endpoint (Ollama, OpenAI, llama.cpp, vLLM) works out of the box via `HttpEmbedderConfig { base_url, model, api_key, dimensions }` with no code changes.
- Adding a new provider means implementing `Embedder` — no changes to `shiro-index`, `shiro-sdk`, or `shiro-store`.
- `StubEmbedder` in `shiro-embed` remains the canonical test double; tests are provider-independent.
- `FlatIndex` in `shiro-embed` stores vectors without encoding provider assumptions; dimension mismatch is detected at upsert time via `dimensions()`.
- Deployments that change provider or model must re-embed all segments, detectable via fingerprint mismatch in `documents.fingerprint`.

## Alternatives Considered

- **Direct Ollama SDK integration**: Tight coupling to Ollama's native client. Breaks non-Ollama deployments, forks the adapter surface, and adds a runtime installation assumption. Rejected.
- **ONNX runtime embedding**: Embeds model inference into the binary, eliminating the external dependency. Introduces a large native dependency, single-runtime assumption, and model distribution complexity. Not appropriate for shiro's scope. Rejected.
- **Provider enum dispatch** (`enum EmbedderKind { Ollama, OpenAI, ... }`): Premature. Encodes provider knowledge into the core and requires code changes for every new provider. The `Embedder` trait already provides open dispatch without an enum. Rejected.

## Non-Goals

- Embedding a model runtime into the shiro binary.
- Providing a model selection or provider discovery UI.
- Auto-detecting available providers on the host system.
- Supporting non-HTTP embedding transports (e.g., shared memory, gRPC) — those require a new `Embedder` implementation, not changes to this decision.
