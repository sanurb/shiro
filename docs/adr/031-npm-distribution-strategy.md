# ADR-031: npm Distribution Strategy

**Status:** Accepted
**Date:** 2026-03-07

## Context

shiro ships a single compiled binary for each supported platform. CI produces one prebuilt binary per target, uploaded as release assets alongside a checksum manifest.

The MCP server subcommand exposes an MCP server over stdio JSON-RPC. Consumers of that interface — VS Code extensions, editor integrations, and similar tooling — live in the Node.js/JS ecosystem and rely on npm as their primary distribution channel. Without an npm package, JS-ecosystem users must compile from source or download binaries manually, both of which are friction-heavy for that audience.

Bundling all platform binaries into a single npm package would produce an artifact far exceeding what any single user needs — most of the download is discarded at install time.

## Decision

The npm package is a **platform-aware downloader**, not a binary bundle. It contains no compiled binary at publish time. At install time, it detects the current platform, fetches the correct prebuilt binary from the release, verifies its integrity, and provides a shim that executes the binary. Node.js is the distribution mechanism only — no Node.js runtime dependency exists for any shiro operation after installation.

**Boundary:** This ADR decides the npm distribution architecture — thin downloader with integrity verification. It does not decide the CI release pipeline implementation, the npm scope/package name, or the set of supported platforms.

**What is canonical:** The prebuilt binary produced by CI for each platform. The npm-installed binary is identical to the binary available through any other installation method.

**What is derived:** The npm package itself (the downloader, the shim, the metadata). These are packaging artifacts, not the product.

**What is allowed:**
- The install script detecting the current platform and architecture to select the correct binary.
- The shim forwarding all arguments and stdio to the binary with no transformation.
- Falling back to a clear error message with manual download instructions when the download or verification fails.

**What is forbidden:**
- Shipping compiled binaries inside the npm package at publish time.
- The shim adding behavior, transforming arguments, or intercepting output. It is a transparent pass-through.
- Completing installation when integrity verification fails. Verification failure is a hard abort.
- Silently succeeding on unsupported platforms. An unsupported platform produces a clear error naming the platform and listing supported alternatives.

**Integrity verification:**
- The install script fetches the checksum manifest from the same release, computes the digest of the downloaded binary, and compares them. A mismatch aborts installation with a non-zero exit and a human-readable error.

**Version identity:**
- The npm package version and the binary release version must match exactly. A version mismatch between the npm package and the downloaded binary is a hard error. The release pipeline publishes to npm only after release assets are available, ensuring the binary exists before the install script can request it.

### Architecture Invariants

- The npm package version and the binary release version match exactly. There is no version mapping or translation — they are the same semver string.
- The downloaded binary must be integrity-verified before installation completes. Verification failure aborts installation — no fallback to an unverified binary.
- The installed binary is byte-identical to the binary available through any other installation method (direct download, package manager, compile from source). There is no npm-specific build or binary variant.
- Node.js is not a runtime dependency for any shiro operation. After installation, the shim's only role is to locate and exec the binary.
- Unsupported platforms produce a clear, actionable error at install time — not silent failure or a cryptic crash at runtime.

### Deliberate Absences

- The set of supported platform targets is not enumerated here — it evolves with CI infrastructure.
- The npm scope and package name are not decided here.
- Behavior when the install script is skipped (e.g., in CI environments that disable post-install hooks) is not prescribed — downstream tools that depend on the package must account for this.
- Mirror or proxy support for environments that cannot reach the release host is not specified.
- Auto-update behavior beyond npm's standard version pinning is not defined.
- Windows platform support is not included in the current distribution path.

## Consequences

- **Product outcome:** JS ecosystem users (VS Code extension authors, MCP tool consumers, editor integration developers) install shiro with a single npm command and get the same binary as users who install through any other method. The installation experience matches ecosystem expectations.
- Only the binary for the user's platform is downloaded — no wasted bandwidth on other platforms' binaries.
- Node.js is not a runtime dependency for any shiro functionality.
- **CI sequencing cost:** The release pipeline must sequence: build binaries → upload release assets → publish npm package. A partial failure (assets uploaded but npm not published, or vice versa) must be detectable and recoverable without re-tagging.
- **Network dependency cost:** Installation requires network access to the release host. Users behind strict firewalls that block the release host must pre-install the binary manually or configure a mirror. The npm package alone is insufficient in air-gapped environments.
- **Post-install hook cost:** Some CI environments and security-conscious configurations disable post-install hooks. Downstream tools that depend on the npm package must account for this and either run the install step explicitly or use a pre-downloaded binary.
- **Maintenance cost:** The downloader, shim, and integrity verification logic must be maintained alongside the binary release process. Changes to release asset naming, checksum format, or hosting require coordinated updates.

## Alternatives Considered

- **Bundle all platform binaries in one npm package:** Simple — no network call at install time, no integrity verification complexity. Would work reliably in air-gapped environments. Rejected: produces an artifact many times larger than needed per user; npm registry size limits make this fragile at scale; every user downloads binaries they discard.
- **Platform-specific scoped packages with optional dependencies:** Publish a separate npm package per platform and let npm's optional dependency resolution pick the correct one. Genuine advantage: no post-install network call — the binary is in the package. Rejected: optional dependency resolution behavior is inconsistent across npm versions and lockfile strategies; platforms can be silently skipped without error; adds multiple packages to maintain and publish atomically with the release.
- **Compile-from-source only (cargo install):** Works well for Rust-ecosystem users; downloads source and compiles with checksum verification. Rejected: not accessible to JS-ecosystem users who do not have a Rust toolchain installed. Remains a valid alternative installation path for Rust users.
- **WebAssembly build:** Compile shiro to WebAssembly and publish as a standard npm package — no platform detection needed. Rejected: significant performance penalty for document parsing and embedding workloads; SQLite has no production-quality WASM target; filesystem and subprocess access required by the embedding subsystem is constrained in WASM environments.
- **Continuous monitoring with rollback:** Distribute via npm without integrity verification; monitor for distribution tampering after the fact. Rejected: supply chain attacks are detected after damage is done. Pre-install verification is strictly superior for a security-relevant tool.

## Non-Goals

- Providing a JS/TypeScript SDK or typed wrapper around the CLI.
- Running shiro in the browser or any environment without filesystem access.
- Surfacing npm-only features or behavior not present in the binary itself.
- Auto-updating the binary outside npm's standard version pinning mechanism.
- Supporting Windows in this distribution path.
