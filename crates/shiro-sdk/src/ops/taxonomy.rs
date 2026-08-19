//! Machine-oriented taxonomy search and bounded graph browsing.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use shiro_core::taxonomy::ConceptId;
use shiro_core::ShiroError;
use shiro_store::Store;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaxonomySearchInput {
    pub query: String,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaxonomyBrowseInput {
    #[serde(default)]
    pub root_concept_id: Option<String>,
    pub max_depth: usize,
    pub max_nodes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaxonomyConceptView {
    pub concept_id: String,
    pub scheme_uri: String,
    pub pref_label: String,
    pub alt_labels: Vec<String>,
    pub definition: Option<String>,
    pub text_fallback: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaxonomyEdgeView {
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaxonomySearchOutput {
    pub concepts: Vec<TaxonomyConceptView>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaxonomyBrowseOutput {
    pub root_concept_id: Option<String>,
    pub truncated: bool,
    pub concepts: Vec<TaxonomyConceptView>,
    pub relations: Vec<TaxonomyEdgeView>,
}

pub fn search(
    store: &Store,
    input: &TaxonomySearchInput,
) -> Result<TaxonomySearchOutput, ShiroError> {
    if input.query.trim().is_empty() || input.limit == 0 {
        return Err(ShiroError::InvalidInput {
            message: "taxonomy search requires a query and positive limit".to_string(),
        });
    }
    Ok(TaxonomySearchOutput {
        concepts: store
            .search_concepts(&input.query, input.limit)?
            .into_iter()
            .map(|concept| concept_view(concept, 0))
            .collect(),
    })
}

pub fn browse(
    store: &Store,
    input: &TaxonomyBrowseInput,
) -> Result<TaxonomyBrowseOutput, ShiroError> {
    if input.max_nodes == 0 {
        return Err(ShiroError::InvalidInput {
            message: "taxonomy browse max_nodes must be positive".to_string(),
        });
    }
    let Some(root) = input.root_concept_id.as_deref() else {
        let concepts = store
            .list_concepts(input.max_nodes.saturating_add(1))?
            .into_iter()
            .collect::<Vec<_>>();
        let truncated = concepts.len() > input.max_nodes;
        return Ok(TaxonomyBrowseOutput {
            root_concept_id: None,
            truncated,
            concepts: concepts
                .into_iter()
                .take(input.max_nodes)
                .map(|concept| concept_view(concept, 0))
                .collect(),
            relations: Vec::new(),
        });
    };
    let root = ConceptId::from_stored(root).map_err(|message| ShiroError::InvalidInput {
        message: message.to_string(),
    })?;
    let mut queue = VecDeque::from([(root.clone(), 0usize)]);
    let mut seen = HashSet::new();
    let mut depths = HashMap::new();
    let mut concepts = Vec::new();
    let mut relations = Vec::new();
    let mut truncated = false;

    while let Some((concept_id, depth)) = queue.pop_front() {
        if !seen.insert(concept_id.clone()) {
            continue;
        }
        if concepts.len() >= input.max_nodes {
            truncated = true;
            break;
        }
        depths.insert(concept_id.clone(), depth);
        concepts.push(store.get_concept(&concept_id)?);
        if depth >= input.max_depth {
            continue;
        }
        let mut outgoing = store.get_concept_relations_any(&concept_id)?;
        outgoing.sort_by(|left, right| {
            left.to
                .as_str()
                .cmp(right.to.as_str())
                .then_with(|| format!("{:?}", left.relation).cmp(&format!("{:?}", right.relation)))
        });
        for relation in outgoing {
            relations.push(TaxonomyEdgeView {
                from: relation.from.as_str().to_string(),
                to: relation.to.as_str().to_string(),
                relation: format!("{:?}", relation.relation).to_uppercase(),
            });
            let adjacent = if relation.from == concept_id {
                relation.to
            } else {
                relation.from
            };
            if !seen.contains(&adjacent) {
                queue.push_back((adjacent, depth + 1));
            }
        }
    }
    relations.retain(|relation| {
        concepts
            .iter()
            .any(|concept| concept.id.as_str() == relation.from)
            && concepts
                .iter()
                .any(|concept| concept.id.as_str() == relation.to)
    });
    let concepts = concepts
        .into_iter()
        .map(|concept| {
            let depth = depths.get(&concept.id).copied().unwrap_or(0);
            concept_view(concept, depth)
        })
        .collect();
    Ok(TaxonomyBrowseOutput {
        root_concept_id: Some(root.as_str().to_string()),
        truncated,
        concepts,
        relations,
    })
}

fn concept_view(concept: shiro_core::taxonomy::Concept, depth: usize) -> TaxonomyConceptView {
    let text_fallback = match &concept.definition {
        Some(definition) => definition.clone(),
        None => concept.pref_label.clone(),
    };
    TaxonomyConceptView {
        concept_id: concept.id.as_str().to_string(),
        scheme_uri: concept.scheme_uri,
        text_fallback,
        pref_label: concept.pref_label,
        alt_labels: concept.alt_labels,
        definition: concept.definition,
        depth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::taxonomy::{Concept, ConceptRelation, SkosRelation};

    #[test]
    fn browse_traverses_incoming_and_outgoing_relations() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(temporary.path())
            .unwrap()
            .join("taxonomy.db");
        let store = Store::open(&path).unwrap();
        let parent = Concept {
            id: ConceptId::new("urn:test", "Parent"),
            scheme_uri: "urn:test".to_string(),
            pref_label: "Parent".to_string(),
            alt_labels: Vec::new(),
            definition: None,
        };
        let child = Concept {
            id: ConceptId::new("urn:test", "Child"),
            scheme_uri: "urn:test".to_string(),
            pref_label: "Child".to_string(),
            alt_labels: Vec::new(),
            definition: None,
        };
        store.put_concept(&parent).unwrap();
        store.put_concept(&child).unwrap();
        store
            .put_concept_relation(&ConceptRelation {
                from: parent.id.clone(),
                to: child.id.clone(),
                relation: SkosRelation::Narrower,
            })
            .unwrap();

        let output = browse(
            &store,
            &TaxonomyBrowseInput {
                root_concept_id: Some(child.id.as_str().to_string()),
                max_depth: 1,
                max_nodes: 10,
            },
        )
        .unwrap();
        assert_eq!(output.concepts.len(), 2);
        assert_eq!(output.relations.len(), 1);
        assert!(output
            .concepts
            .iter()
            .all(|concept| !concept.text_fallback.is_empty()));
    }
}
