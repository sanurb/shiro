//! `shiro taxonomy` — manage SKOS-style taxonomy concepts.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::taxonomy::{Concept, ConceptId, ConceptRelation, SkosRelation};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

pub fn run_add(
    home: &ShiroHome,
    scheme: &str,
    label: &str,
    alt_labels: Option<&str>,
    definition: Option<&str>,
) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let id = ConceptId::new(scheme, label);
    let alts: Vec<String> = alt_labels
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let concept = Concept {
        id: id.clone(),
        scheme_uri: scheme.to_string(),
        pref_label: label.to_string(),
        alt_labels: alts,
        definition: definition.map(String::from),
    };
    let is_new = store.put_concept(&concept)?;
    let result = serde_json::json!({
        "concept_id": id.as_str(),
        "scheme_uri": scheme,
        "pref_label": label,
        "created": is_new,
    });
    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro taxonomy list", "List all concepts"),
            NextAction::simple(
                format!("shiro taxonomy relations {}", id),
                "View concept relations",
            ),
        ],
    })
}

pub fn run_list(home: &ShiroHome, limit: usize) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let concepts = store.list_concepts(limit)?;
    let items: Vec<serde_json::Value> = concepts
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.as_str(),
                "scheme_uri": c.scheme_uri,
                "pref_label": c.pref_label,
                "alt_labels": c.alt_labels,
                "definition": c.definition,
            })
        })
        .collect();
    let count = items.len();
    Ok(CmdOutput {
        result: serde_json::json!({ "concepts": items, "count": count }),
        next_actions: vec![NextAction::simple(
            "shiro taxonomy add --scheme <uri> --label <label>",
            "Add a concept",
        )],
    })
}

pub fn run_relations(home: &ShiroHome, concept_id: &str) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let cid = ConceptId::from_stored(concept_id).map_err(|e| ShiroError::InvalidInput {
        message: e.to_string(),
    })?;
    let concept = store.get_concept(&cid)?;
    let rels = store.get_concept_relations(&cid)?;
    let items: Vec<serde_json::Value> = rels
        .iter()
        .map(|r| {
            serde_json::json!({
                "from": r.from.as_str(),
                "to": r.to.as_str(),
                "relation": r.relation,
            })
        })
        .collect();
    let count = items.len();
    Ok(CmdOutput {
        result: serde_json::json!({
            "concept_id": concept.id.as_str(),
            "pref_label": concept.pref_label,
            "relations": items,
            "count": count,
        }),
        next_actions: vec![NextAction::simple(
            "shiro taxonomy list",
            "List all concepts",
        )],
    })
}

pub fn run_assign(
    home: &ShiroHome,
    doc_id_str: &str,
    concept_id_str: &str,
    confidence: f32,
    source: &str,
) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let doc_id = super::resolve_doc_id(&store, doc_id_str)?;
    let cid = ConceptId::from_stored(concept_id_str).map_err(|e| ShiroError::InvalidInput {
        message: e.to_string(),
    })?;
    let _ = store.get_concept(&cid)?;
    store.assign_concept_to_doc(&doc_id, &cid, confidence, source)?;
    Ok(CmdOutput {
        result: serde_json::json!({
            "doc_id": doc_id.as_str(),
            "concept_id": cid.as_str(),
            "confidence": confidence,
            "source": source,
        }),
        next_actions: vec![
            NextAction::simple(format!("shiro read {}", doc_id), "Read the document"),
            NextAction::simple(
                format!("shiro taxonomy relations {}", cid),
                "View concept relations",
            ),
        ],
    })
}

pub fn run_import(home: &ShiroHome, file: &std::path::Path) -> Result<CmdOutput, ShiroError> {
    let content = std::fs::read_to_string(file).map_err(|e| ShiroError::InvalidInput {
        message: format!("cannot read {}: {e}", file.display()),
    })?;
    #[derive(serde::Deserialize)]
    struct ImportConcept {
        scheme_uri: String,
        pref_label: String,
        #[serde(default)]
        alt_labels: Vec<String>,
        #[serde(default)]
        definition: Option<String>,
        #[serde(default)]
        broader: Vec<String>,
        #[serde(default)]
        narrower: Vec<String>,
        #[serde(default)]
        related: Vec<String>,
    }
    let items: Vec<ImportConcept> =
        serde_json::from_str(&content).map_err(|e| ShiroError::InvalidInput {
            message: format!("invalid JSON in {}: {e}", file.display()),
        })?;
    let store = Store::open(&home.db_path())?;
    let mut created = 0usize;
    let mut skipped = 0usize;
    let mut relations_added = 0usize;
    for item in &items {
        let concept = Concept {
            id: ConceptId::new(&item.scheme_uri, &item.pref_label),
            scheme_uri: item.scheme_uri.clone(),
            pref_label: item.pref_label.clone(),
            alt_labels: item.alt_labels.clone(),
            definition: item.definition.clone(),
        };
        if store.put_concept(&concept)? {
            created += 1;
        } else {
            skipped += 1;
        }
        for target_label in &item.broader {
            let target_id = ConceptId::new(&item.scheme_uri, target_label);
            store.put_concept_relation(&ConceptRelation {
                from: concept.id.clone(),
                to: target_id,
                relation: SkosRelation::Broader,
            })?;
            relations_added += 1;
        }
        for target_label in &item.narrower {
            let target_id = ConceptId::new(&item.scheme_uri, target_label);
            store.put_concept_relation(&ConceptRelation {
                from: concept.id.clone(),
                to: target_id,
                relation: SkosRelation::Narrower,
            })?;
            relations_added += 1;
        }
        for target_label in &item.related {
            let target_id = ConceptId::new(&item.scheme_uri, target_label);
            store.put_concept_relation(&ConceptRelation {
                from: concept.id.clone(),
                to: target_id,
                relation: SkosRelation::Related,
            })?;
            relations_added += 1;
        }
    }
    Ok(CmdOutput {
        result: serde_json::json!({
            "imported": created,
            "skipped": skipped,
            "relations": relations_added,
            "total": items.len(),
        }),
        next_actions: vec![NextAction::simple(
            "shiro taxonomy list",
            "List all concepts",
        )],
    })
}
