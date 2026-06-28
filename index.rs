use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::document::Document;
use crate::error::{NexDbError, NexDbResult};

#[derive(Debug, Clone)]
pub struct FieldIndex {
    name: String,
    field_path: String,
    entries: BTreeMap<String, BTreeSet<String>>,
}

impl FieldIndex {
    pub fn new(name: impl Into<String>, field_path: impl Into<String>) -> Self {
        FieldIndex {
            name: name.into(),
            field_path: field_path.into(),
            entries: BTreeMap::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn field_path(&self) -> &str {
        &self.field_path
    }

    pub fn insert(&mut self, doc_id: &str, doc: &Document) {
        let field_val = doc.get_path(&self.field_path);
        let key = match field_val {
            Some(val) => serde_json::to_string(val).unwrap_or_default(),
            None => return,
        };
        self.entries
            .entry(key)
            .or_default()
            .insert(doc_id.to_string());
    }

    pub fn remove(&mut self, doc_id: &str, doc: &Document) {
        let field_val = doc.get_path(&self.field_path);
        let key = match field_val {
            Some(val) => serde_json::to_string(val).unwrap_or_default(),
            None => return,
        };
        if let Some(ids) = self.entries.get_mut(&key) {
            ids.remove(doc_id);
            if ids.is_empty() {
                self.entries.remove(&key);
            }
        }
    }

    pub fn find_eq(&self, value: &Value) -> Vec<String> {
        let key = serde_json::to_string(value).unwrap_or_default();
        self.entries
            .get(&key)
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn find_range(&self, low: &Value, high: &Value) -> Vec<String> {
        let low_key = serde_json::to_string(low).unwrap_or_default();
        let high_key = serde_json::to_string(high).unwrap_or_default();
        self.entries
            .range(low_key..=high_key)
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }

    pub fn find_prefix(&self, prefix: &str) -> Vec<String> {
        self.entries
            .range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn rebuild(&mut self, docs: &BTreeMap<String, Document>) {
        self.entries.clear();
        for (id, doc) in docs {
            self.insert(id, doc);
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndexManager {
    indexes: Vec<FieldIndex>,
}

impl IndexManager {
    pub fn new() -> Self {
        IndexManager { indexes: Vec::new() }
    }

    pub fn add_index(&mut self, index: FieldIndex) -> NexDbResult<()> {
        let name = index.name().to_string();
        if self.indexes.iter().any(|i| i.name() == name) {
            return Err(NexDbError::Index(format!("index '{}' already exists", name)));
        }
        self.indexes.push(index);
        Ok(())
    }

    pub fn remove_index(&mut self, name: &str) -> Option<FieldIndex> {
        let pos = self.indexes.iter().position(|i| i.name() == name)?;
        Some(self.indexes.remove(pos))
    }

    pub fn get_index(&self, name: &str) -> Option<&FieldIndex> {
        self.indexes.iter().find(|i| i.name() == name)
    }

    pub fn get_index_mut(&mut self, name: &str) -> Option<&mut FieldIndex> {
        self.indexes.iter_mut().find(|i| i.name() == name)
    }

    pub fn indexes(&self) -> &[FieldIndex] {
        &self.indexes
    }

    pub fn insert(&mut self, doc_id: &str, doc: &Document) {
        for index in &mut self.indexes {
            index.insert(doc_id, doc);
        }
    }

    pub fn remove(&mut self, doc_id: &str, doc: &Document) {
        for index in &mut self.indexes {
            index.remove(doc_id, doc);
        }
    }

    pub fn rebuild_all(&mut self, docs: &BTreeMap<String, Document>) {
        for index in &mut self.indexes {
            index.rebuild(docs);
        }
    }

    pub fn clear_all(&mut self) {
        for index in &mut self.indexes {
            index.clear();
        }
    }
}

impl Default for IndexManager {
    fn default() -> Self {
        Self::new()
    }
}
