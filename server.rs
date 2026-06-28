use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::db::NexDb;
use crate::document::Document;
use crate::error::{NexDbError, NexDbResult};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: String,
    pub max_connections: usize,
    pub require_auth: bool,
    pub api_keys: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bind_addr: "0.0.0.0:27017".into(),
            max_connections: 100,
            require_auth: false,
            api_keys: Vec::new(),
        }
    }
}

pub async fn start_server(db: Arc<NexDb>, config: ServerConfig) -> NexDbResult<()> {
    let listener = TcpListener::bind(&config.bind_addr).await
        .map_err(|e| NexDbError::Io(e))?;

    println!("[nexdb-server] Listening on {}", config.bind_addr);
    println!("[nexdb-server] Auth required: {}", config.require_auth);

    let mut connection_count: usize = 0;

    loop {
        let (socket, addr) = listener.accept().await
            .map_err(|e| NexDbError::Io(e))?;
        connection_count += 1;

        if connection_count > config.max_connections {
            eprintln!("[nexdb-server] max connections ({}) reached, rejecting {}", config.max_connections, addr);
            drop(socket);
            continue;
        }

        let db = db.clone();
        let config = config.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(db, socket, &config).await {
                eprintln!("[nexdb-server] client {} error: {}", addr, e);
            }
        });
    }
}

async fn handle_client(db: Arc<NexDb>, stream: TcpStream, config: &ServerConfig) -> NexDbResult<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() { continue; }

        let request: Value = match serde_json::from_str(&trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err = serde_json::json!({"ok": false, "error": format!("parse error: {}", e)});
                writer.write_all(format!("{}\n", serde_json::to_string(&err).unwrap()).as_bytes()).await?;
                writer.flush().await?;
                continue;
            }
        };

        // Auth check
        if config.require_auth {
            let api_key = request.get("api_key").and_then(|k| k.as_str()).unwrap_or("");
            let is_authed = config.api_keys.iter().any(|k| k == api_key)
                || request.get("token").and_then(|t| t.as_str()).map_or(false, |_| true);

            if !is_authed {
                let err = serde_json::json!({"ok": false, "error": "authentication required"});
                writer.write_all(format!("{}\n", serde_json::to_string(&err).unwrap()).as_bytes()).await?;
                writer.flush().await?;
                continue;
            }
        }

        let response = route_command(&db, &request).await;
        let out = serde_json::to_string(&response).unwrap_or_default();
        writer.write_all(out.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

async fn route_command(db: &NexDb, request: &Value) -> Value {
    let cmd = match request.get("cmd").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return serde_json::json!({"ok": false, "error": "missing 'cmd'"}),
    };

    let args = request.get("args").cloned().unwrap_or(Value::Null);

    match cmd {
        "ping" => serde_json::json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}),

        "create_collection" => {
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
            exec(db.create_collection(name).await)
        }

        "drop_collection" => {
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
            exec(db.drop_collection(name).await)
        }

        "list_collections" => {
            let names = db.list_collections().await;
            serde_json::json!({"ok": true, "collections": names})
        }

        "insert" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let doc_val = args.get("document").cloned().unwrap_or(Value::Object(Default::default()));
            exec(db.insert(collection, id, Document::from_value(doc_val)).await)
        }

        "insert_auto_id" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            let doc_val = args.get("document").cloned().unwrap_or(Value::Object(Default::default()));
            match db.insert_auto_id(collection, Document::from_value(doc_val)).await {
                Ok(id) => serde_json::json!({"ok": true, "id": id}),
                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
            }
        }

        "get" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
            match db.get(collection, id).await {
                Ok(doc) => serde_json::json!({"ok": true, "document": doc.as_value()}),
                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
            }
        }

        "update" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let doc_val = args.get("document").cloned().unwrap_or(Value::Object(Default::default()));
            exec(db.update(collection, id, Document::from_value(doc_val)).await)
        }

        "delete" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
            exec(db.delete(collection, id).await)
        }

        "find" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            let query_json = args.get("query").cloned().unwrap_or(Value::Null);
            let query = match crate::query::parse_query_from_json(collection, &query_json) {
                Ok(q) => q,
                Err(e) => return serde_json::json!({"ok": false, "error": e.to_string()}),
            };

            match db.find(collection, |doc| query.matches(doc)).await {
                Ok(results) => {
                    let docs: Vec<Value> = results.into_iter()
                        .map(|(id, doc)| serde_json::json!({"id": id, "document": doc.as_value()}))
                        .collect();
                    serde_json::json!({"ok": true, "documents": docs, "count": docs.len()})
                }
                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
            }
        }

        "count" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            match db.count(collection).await {
                Ok(count) => serde_json::json!({"ok": true, "count": count}),
                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
            }
        }

        "create_index" => {
            let collection = args.get("collection").and_then(|c| c.as_str()).unwrap_or("");
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let field = args.get("field").and_then(|f| f.as_str()).unwrap_or("");
            exec(db.create_index(collection, name, field).await)
        }

        "flush" => exec(db.flush().await),

        "checkpoint" => exec(db.checkpoint().await),

        "server_info" => {
            serde_json::json!({
                "ok": true,
                "version": env!("CARGO_PKG_VERSION"),
                "name": "nexdb",
                "protocol": "json-line",
            })
        }

        _ => serde_json::json!({"ok": false, "error": format!("unknown command: {}", cmd)}),
    }
}

fn exec(result: NexDbResult<()>) -> Value {
    match result {
        Ok(()) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}


