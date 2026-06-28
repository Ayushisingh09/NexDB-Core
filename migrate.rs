use std::path::Path;

use serde_json::Value;

use crate::db::NexDb;
use crate::document::Document;
use crate::error::{NexDbError, NexDbResult};

/// Result of a dump/restore operation
#[derive(Debug)]
pub struct MigrationManifest {
    pub collections: Vec<String>,
    pub total_docs: usize,
}

/// Dump all collections to a directory as JSON files (one per collection)
pub async fn dump(db: &NexDb, out_dir: impl AsRef<Path>) -> NexDbResult<MigrationManifest> {
    let out_dir = out_dir.as_ref().to_path_buf();
    tokio::fs::create_dir_all(&out_dir).await?;

    let collections = db.list_collections().await;
    let mut total_docs = 0;

    for name in &collections {
        let docs = db.dump_collection(name).await?;
        let entries: Vec<Value> = docs.into_iter()
            .map(|(id, doc)| {
                let mut obj = doc.as_value().clone();
                if let Value::Object(ref mut map) = obj {
                    map.insert("_id".to_string(), Value::String(id));
                }
                obj
            })
            .collect();

        let file_path = out_dir.join(format!("{}.json", name));
        let json = serde_json::to_string_pretty(&entries)?;
        tokio::fs::write(&file_path, json).await?;

        total_docs += entries.len();
        println!("  dumped {} documents to {}", entries.len(), file_path.display());
    }

    // Write manifest
    let manifest = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "collections": collections,
        "total_docs": total_docs,
    });
    tokio::fs::write(out_dir.join("_manifest.json"), serde_json::to_string_pretty(&manifest)?).await?;

    Ok(MigrationManifest { collections, total_docs })
}

/// Restore all collections from JSON files in a directory
pub async fn restore(db: &NexDb, in_dir: impl AsRef<Path>) -> NexDbResult<MigrationManifest> {
    let in_dir = in_dir.as_ref().to_path_buf();
    let mut entries = tokio::fs::read_dir(&in_dir).await?;
    let mut collections = Vec::new();
    let mut total_docs = 0;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path.file_stem().and_then(|s| s.to_str()) == Some("_manifest") {
            continue;
        }

        let collection_name = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| NexDbError::Wal(format!("invalid filename: {}", path.display())))?
            .to_string();

        let content = tokio::fs::read_to_string(&path).await?;
        let docs: Vec<Value> = serde_json::from_str(&content)?;

        if docs.is_empty() {
            continue;
        }

        if !db.has_collection(&collection_name).await {
            db.create_collection(&collection_name).await?;
        }

        let mut count = 0;
        for doc_val in &docs {
            let id = doc_val.get("_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            let fields: serde_json::Map<String, Value> = doc_val.as_object()
                .map(|m| m.iter().filter(|(k, _)| *k != "_id").map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            let doc = Document::from_value(Value::Object(fields));

            match id {
                Some(id) => db.insert(&collection_name, &id, doc).await?,
                None => { db.insert_auto_id(&collection_name, doc).await?; }
            }
            count += 1;
        }

        collections.push(collection_name);
        total_docs += count;
        println!("  restored {} documents to {}", count, path.display());
    }

    Ok(MigrationManifest { collections, total_docs })
}

/// Generate a SQL dump for PostgreSQL / MySQL / SQLite compatibility
pub async fn to_sql(db: &NexDb, dialect: &str) -> NexDbResult<String> {
    let collections = db.list_collections().await;
    let mut sql = String::new();

    sql.push_str(&format!("-- NexDb export v{}\n", env!("CARGO_PKG_VERSION")));
    sql.push_str(&format!("-- Generated: {}\n", chrono::Utc::now().to_rfc3339()));
    sql.push_str(&format!("-- Dialect: {}\n\n", dialect));

    for name in &collections {
        let docs = db.dump_collection(name).await?;

        // CREATE TABLE
        let table_name = name.replace('-', "_").replace('.', "_");
        sql.push_str(&format!("-- Collection: {}\n", name));

        // Gather all field names
        let mut all_fields: Vec<String> = Vec::new();
        for (_, doc) in &docs {
            for key in doc.fields() {
                if !all_fields.contains(&key) && key != "_id" {
                    all_fields.push(key);
                }
            }
        }

        let column_defs: Vec<String> = std::iter::once("_id TEXT PRIMARY KEY".to_string())
            .chain(all_fields.iter().map(|f| format!("{} TEXT", f)))
            .collect();
        sql.push_str(&format!("CREATE TABLE IF NOT EXISTS {} (\n  {} \n);\n\n", table_name, column_defs.join(",\n  ")));

        // INSERT rows
        for (id, doc) in &docs {
            let mut col_values: Vec<String> = vec![escape_sql(id)];
            for field in &all_fields {
                let val = doc.get(field).map(|v| match v {
                    Value::String(s) => escape_sql(s),
                    Value::Null => "NULL".to_string(),
                    other => other.to_string(),
                }).unwrap_or_else(|| "NULL".to_string());
                col_values.push(val);
            }
            let cols = std::iter::once("_id".to_string())
                .chain(all_fields.iter().map(|f| format!("\"{}\"", f)))
                .collect::<Vec<_>>();
            sql.push_str(&format!("INSERT INTO \"{}\" ({}) VALUES ({});\n", table_name, cols.join(", "), col_values.join(", ")));
        }
        sql.push('\n');
    }

    Ok(sql)
}

fn escape_sql(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Copy all data from one NexDb to another
pub async fn copy(source: &NexDb, target: &NexDb) -> NexDbResult<MigrationManifest> {
    let collections = source.list_collections().await;
    let mut total_docs = 0;

    for name in &collections {
        if !target.has_collection(name).await {
            target.create_collection(name).await?;
        }

        let docs = source.dump_collection(name).await?;
        for (id, doc) in &docs {
            if target.get(name, id).await.is_err() {
                target.insert(name, id, doc.clone()).await?;
            } else {
                target.update(name, id, doc.clone()).await?;
            }
            total_docs += 1;
        }
        println!("  copied {} documents to collection '{}'", docs.len(), name);
    }

    Ok(MigrationManifest { collections, total_docs })
}

/// Detect the format of a file and import accordingly
pub async fn auto_import(db: &NexDb, collection: &str, file_path: impl AsRef<Path>) -> NexDbResult<usize> {
    let path = file_path.as_ref();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    match ext.as_str() {
        "json" => db.import_json(collection, path).await,
        "csv" => db.import_csv(collection, path).await,
        "ndjson" | "jsonl" => import_ndjson(db, collection, path).await,
        "sql" => {
            eprintln!("SQL import is not yet supported. Use `dump` to export to JSON first.");
            Err(NexDbError::Wal("SQL import not supported".into()))
        }
        _ => Err(NexDbError::Wal(format!("Unsupported file extension: .{}. Supported: .json, .csv, .ndjson", ext))),
    }
}

async fn import_ndjson(db: &NexDb, collection: &str, path: &Path) -> NexDbResult<usize> {
    let content = tokio::fs::read_to_string(path).await?;
    let mut count = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let doc_val: Value = serde_json::from_str(trimmed)?;
        let id = doc_val.get("_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let doc = Document::from_value(doc_val);
        match id {
            Some(id) => db.insert(collection, &id, doc).await?,
            None => { db.insert_auto_id(collection, doc).await?; }
        }
        count += 1;
    }
    Ok(count)
}

/// Remove database files (clean)
pub async fn clean(path: impl AsRef<Path>) -> NexDbResult<usize> {
    let path = path.as_ref();
    let mut removed = 0;

    let db_path = path.with_extension("nexdb");
    if db_path.exists() {
        tokio::fs::remove_file(&db_path).await?;
        removed += 1;
        println!("  removed {}", db_path.display());
    }

    let wal_path = {
        let mut p = path.to_path_buf();
        p.set_extension("nexdb.wal");
        p
    };
    if wal_path.exists() {
        tokio::fs::remove_file(&wal_path).await?;
        removed += 1;
        println!("  removed {}", wal_path.display());
    }

    if removed == 0 {
        println!("  no database files found at {}", path.display());
    }

    Ok(removed)
}

/// Clean all .nexdb and .nexdb.wal files in a directory
pub async fn clean_all(dir: impl AsRef<Path>) -> NexDbResult<usize> {
    let mut removed = 0;
    let mut read_dir = tokio::fs::read_dir(dir.as_ref()).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext == "nexdb" {
                    tokio::fs::remove_file(&path).await?;
                    removed += 1;
                    println!("  removed {}", path.display());

                    let wal = {
                        let mut p = path.clone();
                        p.set_extension("nexdb.wal");
                        p
                    };
                    if wal.exists() {
                        tokio::fs::remove_file(&wal).await?;
                        removed += 1;
                        println!("  removed {}", wal.display());
                    }
                }
            }
        }
    }

    if removed == 0 {
        println!("  no .nexdb files found in {}", dir.as_ref().display());
    }

    Ok(removed)
}
