# ADR-029: Configuration Is Typed, Versioned, and Migrated

**Status:** Accepted
**Date:** 2026-03-07

## Context

shiro's configuration is managed via a TOML file in the user's data directory. As the system grows — embedding provider settings, retrieval tuning parameters, trust zone policies — the configuration surface expands.

Two structural weaknesses exist today:

1. **No schema validation.** Any key-value pair is accepted at write time. Typos (e.g., `embeder.model` instead of `embedder.model`) are silently stored and silently ignored at read time, producing confusing behavior with no error message.
2. **No migration path.** When the config schema changes between shiro releases, users with old config files get undefined behavior — fields may be ignored, defaults may be wrong, or deserialization may fail.

## Decision

Configuration is a **typed schema**, not an arbitrary key-value store. The schema is defined in code and is the **canonical** (the representation that wins when others disagree; the authority from which all others are derived or rebuilt) definition of all configuration options, their types, and their defaults.

**Boundary:** This ADR decides the structural properties of shiro's configuration system — typing, versioning, validation, and migration. It does not decide individual configuration keys, secret management strategy, or the configuration file format (TOML is current but not architecturally load-bearing).

**What is canonical:** The typed configuration schema defined in code. All defaults, types, and valid key names are derived from this schema.

**What is derived:** The user's configuration file. It contains only user overrides — absence of a key means "use the default." Defaults are defined in code, not in a shipped configuration file.

**What is allowed:**
- Adding new configuration keys with defaults (non-breaking).
- Removing or renaming configuration keys with a corresponding migration that preserves user intent.
- Users manually editing the configuration file, subject to validation on next read.
- CLI subcommands for reading and writing individual configuration values.

**What is forbidden:**
- Accepting unknown keys by default. Unknown keys are rejected (fail-closed) to catch typos immediately.
- Storing sensitive values (API keys, tokens) in plaintext in CLI output or logs. Sensitive fields are redacted in any human-readable output.
- Shipping a default configuration file that users are expected to modify. Defaults live in code.
- Skipping migration steps. A config file at version N must pass through every migration function (N→N+1, N+1→N+2, ...) to reach the current version.

**Schema versioning and migration:**
- A version integer is written to the configuration file on every write.
- On startup, shiro reads the version, applies any pending migration functions sequentially, and then validates the result against the current schema.
- Migrations are pure functions from one schema version to the next. They are committed alongside the schema changes they support.

### Architecture Invariants

- The configuration file contains only user overrides. Absence of a key means "use the default." Defaults are defined in code, not in a shipped configuration file.
- A configuration file written by version N of shiro must be readable by version N+1. Forward migration is always supported. Version N+1 reading version N's config applies migration automatically and transparently.
- Unknown keys are rejected at read time. This is fail-closed by design — a typo produces an immediate, actionable error rather than silent misbehavior.
- A configuration file containing invalid syntax (malformed TOML) produces a clear parse error at startup with the file path, line number, and nature of the error. shiro does not start with a corrupt configuration.
- Sensitive values are never present in logs, CLI output, or error messages. They are redacted in any human-readable representation.

### Deliberate Absences

- Individual configuration keys and their semantics are not enumerated here — they evolve with the system.
- Secret management strategy (secure storage, environment variable injection, separate secrets file) is deferred to a future ADR.
- Configuration hot-reload is not supported — shiro reads config at startup and holds it for the process lifetime.
- Per-document or per-collection configuration overrides are not supported — configuration is global to the data directory.
- A forward-compatibility escape hatch (accepting unknown keys during rolling upgrades) is not decided here — the default is fail-closed.
- Backward migration (version N+1 config read by version N) is not guaranteed.

## Consequences

- **Product outcome:** Users upgrading shiro don't lose their configuration. Config files are migrated automatically and transparently. Typos in configuration are caught immediately with actionable error messages, not silently ignored.
- Configuration changes between releases are explicit: each breaking change requires a versioned migration function committed alongside the schema change. This provides an auditable history of configuration evolution.
- CLI output showing configuration is always valid against the current schema and can be round-tripped without data loss.
- **Friction cost:** Adding a new configuration field requires updating the schema, its default, and any migration functions for existing versions. This is intentionally load-bearing friction against ad-hoc configuration sprawl.
- **Migration complexity cost:** The migration function chain must be maintained and tested for every supported version transition. As the version count grows, the testing surface grows linearly.
- **Breaking change cost:** Removing or renaming a key requires writing a migration that maps the old value to the new structure. There is no shortcut — the migration is the compatibility contract.
- **API surface cost:** The fail-closed validation of unknown keys means that configuration files from newer shiro versions are not readable by older versions. Users cannot easily downgrade.

## Alternatives Considered

- **Untyped key-value store (status quo):** Any string key accepted, no validation, no migration. Would avoid migration complexity but guarantees silent misconfiguration as the schema grows. Typos silently ignored. Rejected as the configuration surface expands.
- **Environment variables only:** No persistence between invocations — users must re-supply configuration on every run or manage environment setup externally. Does not compose well with nested configuration (embedding provider settings with multiple fields). Adequate for CI but poor for interactive use. Rejected.
- **Configuration as a SQLite table:** Would integrate with shiro's existing SQLite storage. Provides typed columns and migration via SQL schema changes. Rejected: configuration files are human-readable and human-editable by convention; SQLite requires tooling to inspect or modify. Adds coupling between configuration and the data store — a corrupt database would also lose configuration.
- **JSON configuration:** Functionally equivalent to TOML for this use case but less human-friendly — no comments, stricter syntax, harder to hand-edit. Rejected in favor of TOML, which is already in use.
- **Flat TOML with manual validation:** Defers the problem. Still requires migration logic eventually, but without type safety at the schema boundary. Makes it easy to add unvalidated keys that accumulate tech debt. Rejected.

## Non-Goals

- Not implementing a configuration GUI or TUI.
- Not supporting configuration hot-reload.
- Not implementing per-document or per-collection configuration overrides.
- Not defining a secret management strategy — deferred to a future ADR.
