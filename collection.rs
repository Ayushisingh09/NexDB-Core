use std::collections::BTreeMap;

use crate::document::Document;
use crate::error::{NexDbError, NexDbResult};
use crate::index::IndexManager;

#[derive(Debug, Clone)]
pub struct Collection {
    name: String,
    docs: BTreeMap<String, Document>,
    index_manager: IndexManager,
}

impl Collection {
    pub fn new(name: impl Into<String>) -> Self {
        Collection {
            name: name.into(),
            docs: BTreeMap::new(),
            index_manager: IndexManager::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn insert(&mut self, id: String, doc: Document) -> NexDbResult<Option<Document>> {
        if self.docs.contains_key(&id) {
            return Err(NexDbError::Duplicate(id));
        }
        self.index_manager.insert(&id, &doc);
        Ok(self.docs.insert(id, doc))
    }

    pub fn get(&self, id: &str) -> Option<&Document> {
        self.docs.get(id)
    }

    pub fn update(&mut self, id: &str, doc: Document) -> NexDbResult<Option<Document>> {
        let old = self.docs.get(id).ok_or_else(|| {
            NexDbError::NotFound(format!("document '{}' not found in '{}'", id, self.name))
        })?;

        self.index_manager.remove(id, old);
        self.index_manager.insert(id, &doc);
        Ok(self.docs.insert(id.to_string(), doc))
    }

    pub fn delete(&mut self, id: &str) -> NexDbResult<Option<Document>> {
        let old = self.docs.get(id).ok_or_else(|| {
            NexDbError::NotFound(format!("document '{}' not found in '{}'", id, self.name))
        })?;

        self.index_manager.remove(id, old);
        Ok(self.docs.remove(id))
    }

    pub fn find<F>(&self, predicate: F) -> Vec<(String, Document)>
    where
        F: Fn(&Document) -> bool,
    {
        self.docs
            .iter()
            .filter(|(_, doc)| predicate(doc))
            .map(|(id, doc)| (id.clone(), doc.clone()))
            .collect()
    }

    pub fn count(&self) -> usize {
        self.docs.len()
    }

    pub fn all_docs(&self) -> impl Iterator<Item = (&String, &Document)> {
        self.docs.iter()
    }

    pub fn doc_ids(&self) -> Vec<String> {
        self.docs.keys().cloned().collect()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.docs.contains_key(id)
    }

    pub fn index_manager(&self) -> &IndexManager {
        &self.index_manager
    }

    pub fn index_manager_mut(&mut self) -> &mut IndexManager {
        &mut self.index_manager
    }

    pub fn add_index(&mut self, name: &str, field_path: &str) -> NexDbResult<()> {
        let mut index = crate::index::FieldIndex::new(name, field_path);
        index.rebuild(&self.docs);
        self.index_manager.add_index(index)
    }

    pub fn remove_index(&mut self, name: &str) -> Option<crate::index::FieldIndex> {
        self.index_manager.remove_index(name)
    }

    pub fn find_eq(&self, index_name: &str, value: &serde_json::Value) -> Vec<(String, Document)> {
        let ids = self.index_manager
            .get_index(index_name)
            .map(|idx| idx.find_eq(value))
            .unwrap_or_default();

        ids.iter()
            .filter_map(|id| {
                self.docs.get(id).map(|doc| (id.clone(), doc.clone()))
            })
            .collect()
    }

    pub fn find_range(
        &self,
        index_name: &str,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> Vec<(String, Document)> {
        let ids = self.index_manager
            .get_index(index_name)
            .map(|idx| idx.find_range(low, high))
            .unwrap_or_default();

        ids.iter()
            .filter_map(|id| self.docs.get(id).map(|doc| (id.clone(), doc.clone())))
            .collect()
    }
}
