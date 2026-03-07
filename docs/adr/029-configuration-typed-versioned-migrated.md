# ADR-029: Configuration Is Typed, Versioned, and Migrated

**Status:** Accepted
**Date:** 2026-03-07

## Context

Configuration is managed via `ShiroHome` in `shiro-core::config`. The CLI exposes dotted-key TOML access through `config get` and `config set` subcommands.

The current schema is relatively flat. It will grow as features are added: embedding provider configuration (`HttpEmbedderConfig { base_url, model, api_key, dimensions }`), retrieval tuning parameters (RRF k, BM25 weights), and trust zone policies.

Two structural weaknesses exist today:

1. **No schema validation.** Any key-value pair is accepted at write time. Typos (e.g., `embeder.model`) are silently stored and silently ignored at read time.
2. **No migration path.** When the config schema changes between shiro releases, users with old `config.toml` files get undefined behavior — fields may be ignored, defaults may be wrong, or deserialization may panic.

## Decision

- The config schema MUST be expressed as an explicit Rust struct with `serde` derives — not an arbitrary key-value map. `ShiroHome` and its nested types are the canonical definition.
- Unknown keys are rejected by default (fail-closed). A `--strict=false` flag provides a forward-compatibility escape hatch during rolling upgrades.
- A `config_version` integer field is written to `config.toml` on every write.
- On startup, shiro reads `config_version` and applies any pending migration functions before deserializing into the current struct. Migrations are pure functions: `fn migrate_vN_to_vN1(raw: toml::Value) -> Result<toml::Value>`.
- Sensitive values — specifically `api_key` in `HttpEmbedderConfig` — are either stored in a separate secrets file with restricted permissions or rendered as `[redacted]` in `config show` output. They are never logged.
- Default values are defined in code via `Default` impls, not in a shipped `config.toml`. The config file contains only user overrides. Absence of a key means "use the code default."

## Consequences

- Typos in config keys are caught immediately at `config set` time, not silently ignored at query time.
- Config changes between shiro versions are explicit: each breaking change requires a versioned migration function committed alongside the schema change.
- `config show` output is always valid against the current schema; it can be round-tripped through `config set` without data loss.
- `api_key` and similar sensitive fields are never exposed in CLI output or logs.
- Adding a new config field requires updating the struct, its `Default` impl, and any migration functions for existing versions — this is intentionally load-bearing friction against ad-hoc config sprawl.

## Alternatives Considered

- **Untyped key-value store (current approach):** Error-prone. Typos are silently accepted. No migration story. Rejected as the schema grows.
- **Environment variables only:** No persistence between invocations. Users must re-supply config on every run. Does not compose with `HttpEmbedderConfig` complexity. Rejected.
- **JSON config:** Functionally equivalent to TOML for this use case but less human-friendly (no comments, stricter syntax). Rejected in favor of TOML, which is already in use.
- **Flat TOML with manual validation:** Defers the problem. Still requires migration logic; just without the type safety. Rejected.

## Non-Goals

- Not implementing a configuration GUI or TUI.
- Not supporting config hot-reload — shiro reads config at startup and holds it for the process lifetime.
- Not implementing per-document or per-collection config overrides — config is global to the `ShiroHome` instance.
- Not storing secrets in the primary config file — secret management strategy is deferred to a future ADR.
