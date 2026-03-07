# ADR-031: npm Distribution Strategy for a Rust CLI

**Status:** Accepted
**Date:** 2026-03-07

## Context

shiro ships a single Rust binary (`shiro-cli`) built for four targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `aarch64-apple-darwin`. CI produces one prebuilt binary per target, uploaded as GitHub Release assets alongside a `SHA256SUMS.txt` manifest.

The `shiro mcp` subcommand exposes an MCP server over stdio JSON-RPC. Consumers of that interface — VS Code extensions, Claude Desktop, and similar tooling — live in the Node.js/JS ecosystem and rely on npm as their primary distribution channel. Without an npm package, JS-ecosystem users must install via `cargo install` or manual binary download, both of which are friction-heavy for that audience.

The binary weighs ~15–30 MB compiled. Bundling all four platform binaries into a single npm package would produce a 200 MB+ artifact, most of which is discarded at install time.

Version identity must be unambiguous: the npm package version and the GitHub Release semver tag must match exactly so tooling that pins `@shiro/cli@0.x.y` gets the correct binary without silent mismatches.

## Decision

The npm package is a **thin downloader**, not a bundle. It contains no compiled binary at publish time.

**Package contents:**

- `package.json` — declares `postinstall` hook, `bin` entry pointing to the JS shim, and no native/binary assets
- `install.js` — postinstall script: detects `process.platform` + `process.arch`, resolves the correct asset name, downloads it from `https://github.com/sanurb/shiro/releases/download/v{VERSION}/{ASSET}`, verifies SHA256 against `SHA256SUMS.txt` from the same release, writes the binary to a known path inside the package directory, and sets executable permission
- `shiro.js` — thin JS shim that resolves the installed binary path and `execFileSync`s it, forwarding all `process.argv` and inheriting stdio. This is the `bin` entry; it adds zero runtime overhead beyond process spawn
- `SHA256SUMS.txt` is fetched from the release, not bundled, to keep the npm package small

**SHA256 verification:** `install.js` fetches `SHA256SUMS.txt`, parses the expected digest for the current platform asset, and compares it against the blake3/sha256 digest of the downloaded bytes before writing. Mismatch aborts with a non-zero exit and a human-readable error.

**Fallback on postinstall failure:** if download or verification fails, `install.js` prints the direct GitHub Releases URL for manual download and exits with a non-zero code. The shim (`shiro.js`) detects the binary is absent and prints the same guidance before exiting non-zero, so the failure is surfaced at use-time too, not just install-time.

**Package identity:** `@shiro/cli` on npm. Version field in `package.json` MUST equal the GitHub Release tag (e.g., `0.3.1`). The CI release workflow publishes to npm only after the GitHub Release step succeeds, ensuring the assets exist before `postinstall` can fetch them.

**Node.js is the distribution mechanism only.** Once the binary is installed, no Node.js process is involved in any shiro operation. The shim is the sole Node.js dependency at runtime, and its only job is to exec the binary.

## Consequences

- `npm install @shiro/cli` downloads ~100 KB of JS + ~15–30 MB of platform binary. Other platforms' binaries are never downloaded.
- The installed binary is identical to the artifact produced by CI and available via `cargo install shiro-cli` or direct GitHub Releases download. There is no npm-specific build.
- Node.js is not a runtime dependency for any shiro functionality (`shiro ingest`, `shiro search`, `shiro mcp`, etc.).
- CI must sequence: build → upload GitHub Release assets → publish npm package. A partial failure (assets uploaded, npm not published, or vice versa) must be detectable and re-runnable without re-tagging.
- Users behind strict firewalls that block GitHub must either pre-install the binary manually or configure a mirror; the npm package alone is insufficient in that environment.
- The `postinstall` hook is skipped in some CI environments (`--ignore-scripts`). Downstream tools that bundle `@shiro/cli` must account for this and either run install explicitly or use a pre-downloaded binary.

## Alternatives Considered

**Bundle all platform binaries in one npm package.** Simple, no network call at install time. Rejected: produces a 200 MB+ package; npm registry has a 250 MB soft cap; wastes bandwidth for every user regardless of platform.

**Platform-specific scoped packages with `optionalDependencies`.** Publish `@shiro/cli-linux-x64`, `@shiro/cli-darwin-arm64`, etc., and let npm's optional dependency resolution pick the right one. Rejected: `optionalDependencies` behavior is inconsistent across npm versions and lockfile strategies; platforms can be silently skipped without error; adds four extra packages to maintain and publish atomically.

**`cargo-binstall` only.** Works well for Rust users; downloads prebuilts from GitHub Releases with checksum verification. Rejected: not accessible to JS-ecosystem users who do not have a Rust toolchain installed. cargo-binstall remains a valid alternative install path and is not removed.

**WASM build.** Compile shiro to WebAssembly, publish as a normal npm package. Rejected: significant performance penalty for PDF parsing and embedding workloads; `shiro-store` (rusqlite/SQLite) has no production-quality WASM target; filesystem and subprocess access required by `shiro-embed` (`HttpEmbedder`) is awkward in WASM environments.

## Non-Goals

- Providing a JS/TypeScript SDK or typed wrapper around the CLI. The JS shim does nothing except exec the binary.
- Running shiro in the browser or any environment without filesystem access.
- Surfacing npm-only features or behavior not present in the binary itself.
- Auto-updating the binary outside npm's standard version pinning and `npm update` mechanism.
- Supporting Windows targets in this distribution path (no Windows CI targets are defined; Windows users must use `cargo install` or direct download).
