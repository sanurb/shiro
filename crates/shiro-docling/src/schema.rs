// Schema types are deserialized from external JSON; fields are needed for forward
// compatibility even when not yet consumed by the translation layer.
#![allow(dead_code)]

//! Private serde types for the DoclingDocument JSON schema.
//!
//! These types model only the subset of Docling's output that shiro consumes.
//! They are **crate-private** and must NEVER leak into shiro-core or any
//! other crate's public API. Docling's schema is an external contract — we
//! translate it at the boundary.

use serde::Deserialize;
use std::collections::HashMap;

/// Top-level DoclingDocument (v2 schema).
///
/// Fields not consumed by shiro are omitted via `#[serde(default)]` —
/// forward compatibility with new Docling versions.
#[derive(Debug, Deserialize)]
pub struct DoclingDocument {
    /// Must be "DoclingDocument".
    #[serde(default)]
    pub schema_name: Option<String>,

    /// Semantic version of the Docling schema (e.g. "1.9.0").
    #[serde(default)]
    pub version: Option<String>,

    /// Human-readable document name (often filename without extension).
    #[serde(default)]
    pub name: Option<String>,

    /// All text items (paragraph, heading, caption, footnote, code, etc.).
    #[serde(default)]
    pub texts: Vec<TextItem>,

    /// All table items.
    #[serde(default)]
    pub tables: Vec<TableItem>,

    /// All picture/figure items.
    #[serde(default)]
    pub pictures: Vec<PictureItem>,

    /// Main document body — tree of content references.
    #[serde(default)]
    pub body: NodeItem,

    /// Header/footer/furniture content.
    #[serde(default)]
    pub furniture: NodeItem,

    /// Page-level metadata keyed by page number string.
    #[serde(default)]
    pub pages: HashMap<String, PageItem>,
}

/// A text content item (paragraph, heading, list-item, caption, etc.).
#[derive(Debug, Deserialize)]
pub struct TextItem {
    /// JSON pointer self-reference, e.g. "#/texts/0".
    #[serde(default)]
    pub self_ref: Option<String>,

    /// Structural label: "paragraph", "section_header", "list_item",
    /// "caption", "footnote", "code", "page_header", "page_footer", etc.
    #[serde(default)]
    pub label: String,

    /// The extracted text content.
    #[serde(default)]
    pub text: String,

    /// Original text before normalization.
    #[serde(default)]
    pub orig: Option<String>,

    /// Provenance (page + bounding box).
    #[serde(default)]
    pub prov: Vec<ProvenanceItem>,

    /// Heading level (1-based) when label is section_header.
    #[serde(default)]
    pub level: Option<u32>,
}

/// A table content item.
#[derive(Debug, Deserialize)]
pub struct TableItem {
    /// JSON pointer self-reference, e.g. "#/tables/0".
    #[serde(default)]
    pub self_ref: Option<String>,

    #[serde(default)]
    pub label: String,

    /// Provenance (page + bounding box).
    #[serde(default)]
    pub prov: Vec<ProvenanceItem>,

    /// Table grid data (if available).
    #[serde(default)]
    pub data: Option<TableData>,
}

/// Table grid structure.
#[derive(Debug, Deserialize)]
pub struct TableData {
    /// Table cells in grid order.
    #[serde(default)]
    pub table_cells: Vec<TableCell>,

    /// Number of rows.
    #[serde(default)]
    pub num_rows: usize,

    /// Number of columns.
    #[serde(default)]
    pub num_cols: usize,
}

/// A single cell in a table grid.
#[derive(Debug, Deserialize)]
pub struct TableCell {
    /// Cell text content.
    #[serde(default)]
    pub text: String,

    /// Zero-based row index.
    #[serde(default)]
    pub row_index: usize,

    /// Zero-based column index.
    #[serde(default)]
    pub col_index: usize,

    /// Number of rows this cell spans.
    #[serde(default = "default_one")]
    pub row_span: usize,

    /// Number of columns this cell spans.
    #[serde(default = "default_one")]
    pub col_span: usize,

    /// Whether this is a header cell.
    #[serde(default)]
    pub is_header: bool,
}

fn default_one() -> usize {
    1
}

/// A picture/figure content item.
#[derive(Debug, Deserialize)]
pub struct PictureItem {
    /// JSON pointer self-reference, e.g. "#/pictures/0".
    #[serde(default)]
    pub self_ref: Option<String>,

    #[serde(default)]
    pub label: String,

    /// Provenance (page + bounding box).
    #[serde(default)]
    pub prov: Vec<ProvenanceItem>,
}

/// Tree node in body/furniture. Children reference content items.
#[derive(Debug, Default, Deserialize)]
pub struct NodeItem {
    /// JSON pointer self-reference.
    #[serde(default)]
    pub self_ref: Option<String>,

    /// Structural label (e.g. "group", "ordered_list", "unordered_list").
    #[serde(default)]
    pub label: Option<String>,

    /// Child references — can be content references or nested groups.
    #[serde(default)]
    pub children: Vec<ChildRef>,

    /// Content layer (body vs furniture).
    #[serde(default)]
    pub content_layer: Option<String>,
}

/// A reference to a child node in the document tree.
///
/// This can be either a direct reference (JSON pointer `$ref`) to a content
/// item, or an inline group node with its own children.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ChildRef {
    /// A JSON pointer reference to a content item (e.g. "#/texts/0").
    Ref(RefItem),
    /// An inline group/container node with its own children.
    Inline(NodeItem),
}

/// A JSON pointer reference to a content item.
#[derive(Debug, Deserialize)]
pub struct RefItem {
    /// JSON pointer, e.g. "#/texts/0", "#/tables/1", "#/pictures/2".
    #[serde(rename = "$ref")]
    pub reference: String,
}

/// Provenance: locates a content item on a specific page.
#[derive(Debug, Deserialize)]
pub struct ProvenanceItem {
    /// 1-based page number.
    #[serde(default)]
    pub page_no: Option<u32>,

    /// Bounding box on the page.
    #[serde(default)]
    pub bbox: Option<BoundingBox>,

    /// Character-level start offset in the source.
    #[serde(default)]
    pub charspan: Option<(usize, usize)>,
}

/// Axis-aligned bounding box.
#[derive(Debug, Deserialize)]
pub struct BoundingBox {
    /// Left edge.
    pub l: f64,
    /// Top edge.
    pub t: f64,
    /// Right edge.
    pub r: f64,
    /// Bottom edge.
    pub b: f64,
}

/// Page-level metadata.
#[derive(Debug, Deserialize)]
pub struct PageItem {
    /// 1-based page number.
    #[serde(default)]
    pub page_no: Option<u32>,

    /// Page dimensions.
    #[serde(default)]
    pub size: Option<PageSize>,
}

/// Page dimensions.
#[derive(Debug, Deserialize)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}
