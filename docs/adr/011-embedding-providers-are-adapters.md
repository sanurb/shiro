# ADR-011: Embedding Providers Are Adapters

**Status:** Accepted
**Date:** 2026-03-07

## Context

Ollama is a popular tool for running embedding models locally. Its rise has made local-first embedding operationally attractive, and shiro targets a local-first use case. However, Ollama is not the only viable embedding provider: OpenAI, compatible self-hosted endpoints (llama.cpp server, vLLM, Mistral), and future runtimes are all legitimate deployment targets.

Architecturally coupling shiro to any single provider would mean:

- Assuming a specific runtime is installed on the user's machine.
- Assuming local inference is the only deployment mode.
- Encoding provider-specific behavior into retrieval and indexing logic.

Ollama already exposes an OpenAI-compatible embedding endpoint, which means Ollama support is available through a generic HTTP adapter with no special-casing required. This observation generalizes: any provider that speaks the same HTTP contract is supported without additional code.

## Decision

**Boundary:** This ADR governs the interface between shiro's retrieval/indexing core and all embedding generation. The Embedder trait is the ONLY interface that crosses this boundary. No code above the trait boundary — in the SDK, indexing, storage, or CLI layers — may import, reference, or depend on any specific embedding provider.

**What is canonical:** The Embedder trait is the **canonical** (**canonical**: the representation that wins when others disagree; the authority from which all others are derived or rebuilt) abstraction for embedding generation. All retrieval and indexing logic programs against this trait exclusively.

**What is derived:** Concrete provider adapters (HTTP-based, stub, or future implementations) are **derived** (**derived**: a representation computed from canonical data; rebuildable, not authoritative) in the architectural sense — they are pluggable implementations selected by configuration, not by code changes.

**What is allowed:**

- Any embedding provider that implements the Embedder trait is a valid adapter.
- Provider selection is a configuration concern, resolved at application startup.
- A test double implementing the Embedder trait is the standard mechanism for provider-independent testing.

**What is forbidden:**

- No direct dependency from the retrieval core (SDK, indexing, storage) to any specific provider.
- No assumption that any particular provider is installed on the host system.
- No assumption that local inference is the only deployment mode.
- No embedding identity inferred from model name alone — dimensions and a versioned fingerprint together constitute identity (see ADR-012).

All embedding outputs must carry provider, model, and fingerprint metadata to enable correctness checks across provider changes. Embedding identity must not be inferred from model name alone; dimension count and versioned fingerprint together constitute identity (detailed in ADR-012).

### Architecture Invariants

- The Embedder trait is the sole abstraction boundary. Adding a new provider means implementing this trait. No changes to indexing, retrieval, or storage logic are required or permitted.
- Switching providers requires re-embedding all content. There is no migration path that preserves vectors across provider changes. This is a fundamental constraint: different providers produce incompatible vector spaces, and no transformation can reliably map between them.
- All vectors in a single index must share one embedding fingerprint (enforced by ADR-012). Mixed-provider indices are forbidden.

### Deliberate Absences

- No provider is architecturally privileged. The system does not prefer, recommend, or default to any specific provider at the architectural level. Default provider selection is a CLI/configuration concern.
- No assumption that local inference is always available or preferred.
- No provider discovery or auto-detection mechanism.
- No embedding transport protocol is mandated beyond the trait contract. HTTP is the current implementation; other transports (gRPC, in-process) require a new Embedder implementation, not changes to this decision.

## Consequences

- **Product outcome:** Users can switch from Ollama to OpenAI (or any compatible provider) by changing configuration, not code. A local-first user who later adopts a cloud provider, or vice versa, faces a re-embedding step but no architectural migration.
- Any OpenAI-compatible embedding endpoint works with no code changes — only configuration (endpoint URL, model name, API key, dimensions).
- Adding a new provider means implementing a single trait. No changes to indexing, storage, or retrieval logic.
- **Migration cost:** Switching providers requires re-embedding all indexed content. For large knowledge bases, this is a significant time and compute cost. The system detects this via fingerprint mismatch (ADR-012) and blocks mixed-provider search.
- **Complexity cost:** The trait boundary introduces an indirection layer. Provider-specific diagnostics (rate limiting, authentication errors, model availability) must be surfaced through a generic error interface, which can obscure root causes.
- **Testing cost:** A test double must faithfully implement the Embedder trait including fingerprint generation. Tests that depend on specific vector values are inherently provider-coupled and should be avoided.
- **API surface cost:** The Embedder trait is a public contract. Changes to it (new required methods, signature changes) are breaking for all provider implementations.

## Alternatives Considered

- **Direct Ollama SDK integration.** Tight coupling to Ollama's native client. Breaks non-Ollama deployments, forks the adapter surface, and adds a runtime installation assumption. Choosing this means every future provider requires a separate integration path, and Ollama becomes a hard dependency for development and testing.
- **ONNX runtime embedding.** Embeds model inference directly into the binary, eliminating the external provider dependency entirely. This has a genuine advantage: fully offline operation with no network dependency and predictable latency. However, it introduces a large native dependency, constrains model selection to ONNX-exported models, and adds model distribution complexity (bundling or downloading weights). Not appropriate for shiro's current scope, but remains a plausible future Embedder implementation behind the same trait boundary.
- **Provider enum dispatch.** Encodes provider knowledge into the core via an enumeration of known providers. Requires code changes for every new provider. The Embedder trait already provides open dispatch without an enum, making this strictly less flexible with no compensating benefit.

## Non-Goals

- Embedding a model runtime into the shiro binary.
- Providing a model selection or provider discovery UI.
- Auto-detecting available providers on the host system.
- Supporting non-HTTP embedding transports in this decision — those are future Embedder implementations, not changes to the adapter architecture.
