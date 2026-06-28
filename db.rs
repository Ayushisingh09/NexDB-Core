use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::collection::Collection;
use crate::document::Document;
use crate::error::{NexDbError, NexDbResult};
use crate::wal::{WalEntry, WalReader, WalWriter};

pub struct NexDb {
    collections: RwLock<HashMap<String, Arc<RwLock<Collection>>>>,
    wal: Mutex<WalWriter>,
    path: PathBuf,
}

impl NexDb {
    pub async fn open(path: impl AsRef<Path>) -> NexDbResult<Self> {
        let path = path.as_ref().to_path_buf();
        let wal_writer = WalWriter::new(&path).await?;

        let db = NexDb {
            collections: RwLock::new(HashMap::new()),
            wal: Mutex::new(wal_writer),
            path: path.clone(),
        };

        let mut wal_reader = WalReader::new(&path);
        wal_reader.replay(|entry| {
            db.replay_entry(&entry)
        }).await?;

        Ok(db)
    }

    fn replay_entry(&self, entry: &WalEntry) -> NexDbResult<()> {
        let mut collections = self.collections.try_write()
            .map_err(|_| NexDbError::Wal("failed to acquire collections lock during replay".into()))?;

        let coll = collections.entry(entry.collection.clone()).or_insert_with(|| {
            Arc::new(RwLock::new(Collection::new(&entry.collection)))
        });

        let mut coll = coll.try_write()
            .map_err(|_| NexDbError::Wal("failed to acquire collection lock during replay".into()))?;

        match entry.operation {
            crate::wal::WalOperation::Insert => {
                if let Some(ref doc_val) = entry.document {
                    let doc = Document::from_value(doc_val.clone());
                    if !coll.contains(&entry.doc_id) {
                        coll.insert(entry.doc_id.clone(), doc).ok();
                    }
                }
            }
            crate::wal::WalOperation::Update => {
                if let Some(ref doc_val) = entry.document {
                    if coll.contains(&entry.doc_id) {
                        let doc = Document::from_value(doc_val.clone());
                        coll.update(&entry.doc_id, doc).ok();
                    }
                }
            }
            crate::wal::WalOperation::Delete => {
                if coll.contains(&entry.doc_id) {
                    coll.delete(&entry.doc_id).ok();
                }
            }
        }

        Ok(())
    }

    pub async fn create_collection(&self, name: &str) -> NexDbResult<()> {
        let mut collections = self.collections.write().await;
        if collections.contains_key(name) {
            return Err(NexDbError::CollectionAlreadyExists(name.to_string()));
        }
        collections.insert(name.to_string(), Arc::new(RwLock::new(Collection::new(name))));
        Ok(())
    }

    pub async fn drop_collection(&self, name: &str) -> NexDbResult<()> {
        let mut collections = self.collections.write().await;
        collections.remove(name)
            .ok_or_else(|| NexDbError::CollectionNotFound(name.to_string()))?;
        Ok(())
    }

    pub async fn list_collections(&self) -> Vec<String> {
        let collections = self.collections.read().await;
        let mut names: Vec<String> = collections.keys().cloned().collect();
        names.sort();
        names
    }

    pub async fn has_collection(&self, name: &str) -> bool {
        let collections = self.collections.read().await;
        collections.contains_key(name)
    }

    async fn get_collection_arc(&self, name: &str) -> NexDbResult<Arc<RwLock<Collection>>> {
        let collections = self.collections.read().await;
        collections.get(name)
            .cloned()
            .ok_or_else(|| NexDbError::CollectionNotFound(name.to_string()))
    }

    async fn write_wal(&self, entry: &WalEntry) -> NexDbResult<()> {
        let mut wal = self.wal.lock().await;
        wal.append(entry).await
    }

    pub async fn insert(&self, collection: &str, id: &str, doc: Document) -> NexDbResult<()> {
        let coll_arc = self.get_collection_arc(collection).await?;

        let entry = WalEntry::new_insert(collection, id, doc.as_value());

        self.write_wal(&entry).await?;

        let mut coll = coll_arc.write().await;
        coll.insert(id.to_string(), doc)?;
        Ok(())
    }

    pub async fn insert_auto_id(&self, collection: &str, doc: Document) -> NexDbResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.insert(collection, &id, doc).await?;
        Ok(id)
    }

    pub async fn get(&self, collection: &str, id: &str) -> NexDbResult<Document> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let coll = coll_arc.read().await;
        coll.get(id)
            .cloned()
            .ok_or_else(|| NexDbError::NotFound(format!("document '{}' not found in '{}'", id, collection)))
    }

    pub async fn update(&self, collection: &str, id: &str, doc: Document) -> NexDbResult<()> {
        let coll_arc = self.get_collection_arc(collection).await?;

        let entry = WalEntry::new_update(collection, id, doc.as_value());

        self.write_wal(&entry).await?;

        let mut coll = coll_arc.write().await;
        coll.update(id, doc)?;
        Ok(())
    }

    pub async fn delete(&self, collection: &str, id: &str) -> NexDbResult<()> {
        let coll_arc = self.get_collection_arc(collection).await?;

        let entry = WalEntry::new_delete(collection, id);

        self.write_wal(&entry).await?;

        let mut coll = coll_arc.write().await;
        coll.delete(id)?;
        Ok(())
    }

    pub async fn find<F>(&self, collection: &str, predicate: F) -> NexDbResult<Vec<(String, Document)>>
    where
        F: Fn(&Document) -> bool,
    {
        let coll_arc = self.get_collection_arc(collection).await?;
        let coll = coll_arc.read().await;
        Ok(coll.find(predicate))
    }

    pub async fn count(&self, collection: &str) -> NexDbResult<usize> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let coll = coll_arc.read().await;
        Ok(coll.count())
    }

    pub async fn find_eq(
        &self,
        collection: &str,
        index_name: &str,
        value: &serde_json::Value,
    ) -> NexDbResult<Vec<(String, Document)>> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let coll = coll_arc.read().await;
        Ok(coll.find_eq(index_name, value))
    }

    pub async fn find_range(
        &self,
        collection: &str,
        index_name: &str,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> NexDbResult<Vec<(String, Document)>> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let coll = coll_arc.read().await;
        Ok(coll.find_range(index_name, low, high))
    }

    pub async fn create_index(
        &self,
        collection: &str,
        index_name: &str,
        field_path: &str,
    ) -> NexDbResult<()> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let mut coll = coll_arc.write().await;
        coll.add_index(index_name, field_path)
    }

    pub async fn drop_index(&self, collection: &str, index_name: &str) -> NexDbResult<()> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let mut coll = coll_arc.write().await;
        coll.remove_index(index_name)
            .ok_or_else(|| NexDbError::NotFound(format!("index '{}' not found in '{}'", index_name, collection)))?;
        Ok(())
    }

    pub async fn flush(&self) -> NexDbResult<()> {
        let mut wal = self.wal.lock().await;
        wal.flush().await
    }

    /// WAL checkpoint: snapshot all collections & truncate WAL
    pub async fn checkpoint(&self) -> NexDbResult<()> {
        let mut wal = self.wal.lock().await;
        let collections = self.collections.read().await;
        let mut snapshot = HashMap::new();
        for (name, coll_arc) in collections.iter() {
            let coll = coll_arc.read().await;
            let docs: HashMap<String, Value> = coll.all_docs()
                .map(|(id, doc)| (id.clone(), doc.as_value().clone()))
                .collect();
            snapshot.insert(name.clone(), docs);
        }
        wal.checkpoint(snapshot).await
    }

    /// Export a collection as JSON array (one file)
    pub async fn export_json(&self, collection: &str, file_path: impl AsRef<Path>) -> NexDbResult<()> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let coll = coll_arc.read().await;
        let docs: Vec<serde_json::Value> = coll.all_docs().map(|(id, doc)| {
                let mut obj = doc.as_value().clone();
                if let Value::Object(ref mut map) = obj {
                    map.insert("_id".to_string(), Value::String(id.clone()));
                }
                obj
            })
            .collect();
        let json = serde_json::to_string_pretty(&docs)?;
        tokio::fs::write(file_path.as_ref(), json).await?;
        Ok(())
    }

    /// Import documents from a JSON file (array of objects with optional _id)
    pub async fn import_json(&self, collection: &str, file_path: impl AsRef<Path>) -> NexDbResult<usize> {
        let content = tokio::fs::read_to_string(file_path.as_ref()).await?;
        let docs: Vec<serde_json::Value> = serde_json::from_str(&content)?;
        let mut count = 0;
        for doc_val in docs {
            let id = doc_val.get("_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let fields: serde_json::Map<String, Value> = doc_val.as_object()
                .map(|m| m.iter().filter(|(k, _)| *k != "_id").map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            let doc = Document::from_value(Value::Object(fields));
            match id {
                Some(id) => self.insert(collection, &id, doc).await?,
                None => { self.insert_auto_id(collection, doc).await?; }
            }
            count += 1;
        }
        Ok(count)
    }

    /// Export collection as CSV
    pub async fn export_csv(&self, collection: &str, file_path: impl AsRef<Path>) -> NexDbResult<()> {
        let coll_arc = self.get_collection_arc(collection).await?;
        let coll = coll_arc.read().await;
        let docs: Vec<(String, Document)> = coll.all_docs().map(|(k,v)| (k.clone(), v.clone())).collect();
        if docs.is_empty() {
            tokio::fs::write(file_path.as_ref(), "").await?;
            return Ok(());
        }

        let mut all_keys: Vec<String> = Vec::new();
        for (_, doc) in &docs {
            for key in doc.fields() {
                if !all_keys.contains(&key) {
                    all_keys.push(key);
                }
            }
        }

        let mut file = std::fs::File::create(file_path.as_ref())?;
        // header
        writeln!(file, "{}", all_keys.join(","))?;
        // rows
        for (_id, doc) in &docs {
            let row: Vec<String> = all_keys.iter().map(|key| {
                doc.get(key).map(|v| {
                    match v {
                        Value::String(s) => format!("\"{}\"", s.replace('"', "\"\"")),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    }
                }).unwrap_or_default()
            }).collect();
            writeln!(file, "{}", row.join(","))?;
        }
        Ok(())
    }

    /// Import documents from CSV (first row = headers)
    pub async fn import_csv(&self, collection: &str, file_path: impl AsRef<Path>) -> NexDbResult<usize> {
        let file = std::fs::File::open(file_path.as_ref())?;
        let reader = std::io::BufReader::new(file);
        let mut lines = reader.lines();
        let header = lines.next()
            .ok_or_else(|| NexDbError::Wal("empty CSV file".into()))??;
        let headers: Vec<&str> = header.split(',').map(|s| s.trim()).collect();
        let mut count = 0;

        for line in lines {
            let line = line?;
            if line.trim().is_empty() { continue; }
            let mut map = serde_json::Map::new();
            for (i, value_raw) in line.split(',').enumerate() {
                if i >= headers.len() { break; }
                let val = value_raw.trim().trim_matches('"');
                map.insert(headers[i].to_string(), Value::String(val.to_string()));
            }
            let doc = Document::from_value(Value::Object(map));
            self.insert_auto_id(collection, doc).await?;
            count += 1;
        }
        Ok(count)
    }

    /// Dump all docs from a single collection
    pub async fn dump_collection(&self, name: &str) -> NexDbResult<Vec<(String, Document)>> {
        let coll_arc = self.get_collection_arc(name).await?;
        let coll = coll_arc.read().await;
        Ok(coll.all_docs().map(|(k,v)| (k.clone(), v.clone())).collect())
    }

    /// Dump all docs from all collections (for full backup or checkpoint)
    pub async fn dump_all_collections(&self) -> HashMap<String, Vec<(String, Document)>> {
        let collections = self.collections.read().await;
        let mut out = HashMap::new();
        for (name, coll_arc) in collections.iter() {
            let coll = coll_arc.read().await;
            let docs: Vec<(String, Document)> = coll.all_docs().map(|(k,v)| (k.clone(), v.clone())).collect();
            out.insert(name.clone(), docs);
        }
        out
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for NexDb {
    fn drop(&mut self) {
        // In a real system, we'd block here to flush the WAL.
        // For the async Drop problem, we'd use a shutdown channel instead.
    }
}
