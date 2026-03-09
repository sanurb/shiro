//! Docling-based structured document parser adapter for shiro.
//!
//! This crate provides [`DoclingParser`], an implementation of
//! [`shiro_core::ports::Parser`] that uses the [Docling](https://github.com/docling-project/docling)
//! CLI to extract structured content from PDFs and other document formats.
//!
//! # Architecture
//!
//! Docling is invoked as a subprocess — its Python runtime and model weights
//! are external to shiro. The adapter translates Docling's `DoclingDocument`
//! JSON into shiro's canonical IR ([`shiro_core::ir::Document`] +
//! [`shiro_core::ir::BlockGraph`]) at the crate boundary.
//!
//! **Docling types are private to this crate.** They never leak into
//! shiro-core, shiro-store, shiro-sdk, or any other workspace crate.
//!
//! # Usage
//!
//! ```rust,no_run
//! use shiro_core::ports::Parser;
//! use shiro_docling::DoclingParser;
//!
//! let parser = DoclingParser::new();
//! // Requires `docling` CLI on $PATH.
//! // let doc = parser.parse("report.pdf", &std::fs::read("report.pdf").unwrap()).unwrap();
//! ```
//!
//! # Processing Fingerprint
//!
//! The parser reports:
//! - `name()` → `"docling"`
//! - `version()` → adapter version (bumped on any output-affecting change)
//!
//! Per ADR-004, changing the Docling CLI version or the translation layer
//! requires bumping the adapter version to trigger re-ingestion.

mod parser;
#[doc(hidden)]
pub mod schema;
#[doc(hidden)]
pub mod translate;

pub use parser::DoclingParser;

/// Test-only re-exports for integration tests.
///
/// These are NOT part of the public API — they exist solely to allow
/// fixture-backed tests to exercise the translation layer without
/// requiring the `docling` binary.
#[doc(hidden)]
pub mod __test_support {
    pub use crate::schema::DoclingDocument;
    pub use crate::translate::translate;
}
