use std::collections::HashMap;
use std::sync::RwLock;

use shiro_core::ports::DocumentStore;
use shiro_core::{DocId, Document, ShiroError};

pub struct MemoryStore {
    docs: RwLock<HashMap<DocId, Document>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            docs: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentStore for MemoryStore {
    fn put(&self, doc: &Document) -> Result<(), ShiroError> {
        let mut map = self.docs.write().map_err(|e| ShiroError::Store {
            message: format!("lock poisoned: {e}"),
        })?;
        map.insert(doc.id.clone(), doc.clone());
        Ok(())
    }

    fn get(&self, id: &DocId) -> Result<Document, ShiroError> {
        let map = self.docs.read().map_err(|e| ShiroError::Store {
            message: format!("lock poisoned: {e}"),
        })?;
        map.get(id)
            .cloned()
            .ok_or_else(|| ShiroError::NotFound(id.clone()))
    }

    fn list(&self) -> Result<Vec<DocId>, ShiroError> {
        let map = self.docs.read().map_err(|e| ShiroError::Store {
            message: format!("lock poisoned: {e}"),
        })?;
        Ok(map.keys().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use shiro_core::Metadata;

    fn make_doc(content: &[u8]) -> Document {
        Document {
            id: DocId::from_content(content),
            metadata: Metadata {
                title: None,
                source: Utf8PathBuf::from("test.txt"),
            },
            segments: vec![],
            blocks: None,
        }
    }

    #[test]
    fn put_and_get() {
        let store = MemoryStore::new();
        let doc = make_doc(b"test");
        store.put(&doc).unwrap();
        let retrieved = store.get(&doc.id).unwrap();
        assert_eq!(retrieved.id, doc.id);
    }

    #[test]
    fn get_missing() {
        let store = MemoryStore::new();
        let id = DocId::from_content(b"nonexistent");
        let err = store.get(&id).unwrap_err();
        assert!(matches!(err, ShiroError::NotFound(_)));
    }

    #[test]
    fn list_documents() {
        let store = MemoryStore::new();
        store.put(&make_doc(b"one")).unwrap();
        store.put(&make_doc(b"two")).unwrap();
        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn put_idempotent() {
        let store = MemoryStore::new();
        let doc = make_doc(b"same");
        assert!(store.put(&doc).is_ok());
        assert!(store.put(&doc).is_ok());
        assert_eq!(store.list().unwrap().len(), 1);
    }
}
