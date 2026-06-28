use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::error::NexDbResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    #[serde(rename = "ts")]
    pub timestamp: i64,
    #[serde(rename = "c")]
    pub collection: String,
    #[serde(rename = "op")]
    pub operation: WalOperation,
    #[serde(rename = "id")]
    pub doc_id: String,
    #[serde(rename = "doc")]
    pub document: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalOperation {
    #[serde(rename = "insert")]
    Insert,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "create_collection")]
    CreateCollection,
    #[serde(rename = "drop_collection")]
    DropCollection,
}

impl WalEntry {
    pub fn new_insert(collection: &str, doc_id: &str, document: &Value) -> Self {
        WalEntry {
            timestamp: Utc::now().timestamp(),
            collection: collection.to_string(),
            operation: WalOperation::Insert,
            doc_id: doc_id.to_string(),
            document: Some(document.clone()),
        }
    }

    pub fn new_update(collection: &str, doc_id: &str, document: &Value) -> Self {
        WalEntry {
            timestamp: Utc::now().timestamp(),
            collection: collection.to_string(),
            operation: WalOperation::Update,
            doc_id: doc_id.to_string(),
            document: Some(document.clone()),
        }
    }

    pub fn new_delete(collection: &str, doc_id: &str) -> Self {
        WalEntry {
            timestamp: Utc::now().timestamp(),
            collection: collection.to_string(),
            operation: WalOperation::Delete,
            doc_id: doc_id.to_string(),
            document: None,
        }
    }

    pub fn new_create_collection(collection: &str) -> Self {
        WalEntry {
            timestamp: Utc::now().timestamp(),
            collection: collection.to_string(),
            operation: WalOperation::CreateCollection,
            doc_id: String::new(),
            document: None,
        }
    }

    pub fn new_drop_collection(collection: &str) -> Self {
        WalEntry {
            timestamp: Utc::now().timestamp(),
            collection: collection.to_string(),
            operation: WalOperation::DropCollection,
            doc_id: String::new(),
            document: None,
        }
    }
}

pub struct WalWriter {
    file: tokio::fs::File,
    path: PathBuf,
}

impl WalWriter {
    pub async fn new(path: impl AsRef<Path>) -> NexDbResult<Self> {
        if let Some(parent) = path.as_ref().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())
            .await?;

        Ok(WalWriter {
            file,
            path: path.as_ref().to_path_buf(),
        })
    }

    pub async fn append(&mut self, entry: &WalEntry) -> NexDbResult<()> {
        let json = serde_json::to_string(entry)?;
        let mut line = json.into_bytes();
        line.push(b'\n');
        self.file.write_all(&line).await?;
        self.file.sync_all().await?;
        Ok(())
    }

    pub async fn flush(&mut self) -> NexDbResult<()> {
        self.file.sync_all().await?;
        Ok(())
    }

    /// Checkpoint: truncate the WAL and rewrite with all current docs as inserts
    pub async fn checkpoint(&mut self, snapshot: HashMap<String, HashMap<String, Value>>) -> NexDbResult<()> {
        // Re-open in write (truncate) mode
        self.file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
            .await?;

        for (collection, docs) in &snapshot {
            for (doc_id, doc_val) in docs {
                let entry = WalEntry::new_insert(collection, doc_id, doc_val);
                let json = serde_json::to_string(&entry)?;
                let mut line = json.into_bytes();
                line.push(b'\n');
                self.file.write_all(&line).await?;
            }
        }
        self.file.sync_all().await?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct WalReader {
    path: PathBuf,
}

impl WalReader {
    pub fn new(path: impl AsRef<Path>) -> Self {
        WalReader {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub async fn exists(&self) -> bool {
        tokio::fs::try_exists(&self.path).await.unwrap_or(false)
    }

    pub async fn replay<F>(&mut self, mut apply: F) -> NexDbResult<()>
    where
        F: FnMut(WalEntry) -> NexDbResult<()>,
    {
        if !self.exists().await {
            return Ok(());
        }

        let file = tokio::fs::File::open(&self.path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: WalEntry = serde_json::from_str(trimmed)?;
            apply(entry)?;
        }

        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub async fn wal_exists(path: impl AsRef<Path>) -> bool {
    tokio::fs::try_exists(path.as_ref()).await.unwrap_or(false)
}
