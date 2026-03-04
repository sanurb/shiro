// TODO: query FtsIndex + VectorStore, merge and rank results. Acceptance: returns ranked segments matching query.

use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

#[derive(Serialize)]
pub(crate) struct SearchResult {
    segment_id: String,
    doc_id: String,
    content: String,
    score: f64,
}

impl fmt::Display for SearchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{:.4}] {} (doc={}, seg={})",
            self.score, self.content, self.doc_id, self.segment_id
        )
    }
}

#[derive(Serialize)]
pub(crate) struct SearchData {
    query: String,
    results: Vec<SearchResult>,
}

impl fmt::Display for SearchData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "search: \"{}\"", self.query)?;
        if self.results.is_empty() {
            write!(f, "  (no results)")
        } else {
            for r in &self.results {
                writeln!(f, "  {r}")?;
            }
            Ok(())
        }
    }
}

pub(crate) fn run(query: &str, limit: usize, json: bool) -> i32 {
    let _ = limit; // TODO: pass to index queries

    let data = SearchData {
        query: query.to_owned(),
        results: Vec::new(),
    };

    let next_actions = [NextAction {
        command: "shiro add <path>".into(),
        description: "Add documents first".into(),
    }];

    envelope::print_success("shiro search", &data, &next_actions, json)
}
