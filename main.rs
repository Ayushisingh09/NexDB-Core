use std::path::Path;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use nexdb::{Document, NexDb, NexDbResult};
use nexdb::server;

fn usage() -> ! {
    eprintln!("NexDb v{} - Embeddable Document Database", nexdb::version());
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  nexdb repl <database.nexdb>            Start interactive REPL");
    eprintln!("  nexdb serve <database.nexdb> [--port N] Start TCP server");
    eprintln!("  nexdb insert <db> <col> <id> <json>    Insert a document");
    eprintln!("  nexdb get <db> <col> <id>              Get a document");
    eprintln!("  nexdb update <db> <col> <id> <json>    Update a document");
    eprintln!("  nexdb delete <db> <col> <id>           Delete a document");
    eprintln!("  nexdb count <db> <col>                 Count documents");
    eprintln!("  nexdb collections <db>                 List collections");
    eprintln!("  nexdb create-collection <db> <col>     Create a collection");
    eprintln!("  nexdb drop-collection <db> <col>       Drop a collection");
    eprintln!("  nexdb flush <db>                       Flush WAL to disk");
    eprintln!("  nexdb checkpoint <db>                  WAL checkpoint (snapshot)");
    eprintln!("  nexdb import <db> <col> <file.json>    Import JSON");
    eprintln!("  nexdb export <db> <col> <file.json>    Export JSON");
    eprintln!("  nexdb import-csv <db> <col> <file.csv> Import CSV");
    eprintln!("  nexdb export-csv <db> <col> <file.csv> Export CSV");
    eprintln!("  nexdb completions <shell>              Generate shell completions");
    eprintln!();
    eprintln!("SHELLS: bash, zsh, fish, powershell, elvish");
    std::process::exit(1);
}

#[tokio::main]
async fn main() -> NexDbResult<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }

    match args[1].as_str() {
        "repl" => {
            if args.len() < 3 { usage(); }
            run_repl(Path::new(&args[2])).await
        }
        "serve" => {
            if args.len() < 3 { usage(); }
            let port = args.iter().position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(27017);
            let db = NexDb::open(&args[2]).await?;
            let db = std::sync::Arc::new(db);
            let config = server::ServerConfig {
                bind_addr: format!("0.0.0.0:{}", port),
                ..Default::default()
            };
            server::start_server(db, config).await
        }
        "insert" => {
            if args.len() < 6 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            let doc = Document::from_json(&args[5])?;
            db.insert(&args[3], &args[4], doc).await?;
            println!(r#"{{"ok":true}}"#);
            Ok(())
        }
        "get" => {
            if args.len() < 5 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            match db.get(&args[3], &args[4]).await {
                Ok(doc) => println!("{}", doc.to_json()),
                Err(e) => eprintln!("{}", e),
            }
            Ok(())
        }
        "update" => {
            if args.len() < 6 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            let doc = Document::from_json(&args[5])?;
            db.update(&args[3], &args[4], doc).await?;
            println!(r#"{{"ok":true}}"#);
            Ok(())
        }
        "delete" => {
            if args.len() < 5 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            db.delete(&args[3], &args[4]).await?;
            println!(r#"{{"ok":true}}"#);
            Ok(())
        }
        "count" => {
            if args.len() < 4 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            let count = db.count(&args[3]).await?;
            println!("{}", count);
            Ok(())
        }
        "collections" | "list" => {
            if args.len() < 3 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            let names = db.list_collections().await;
            println!("{}", serde_json::to_string_pretty(&names).unwrap());
            Ok(())
        }
        "create-collection" => {
            if args.len() < 4 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            db.create_collection(&args[3]).await?;
            println!(r#"{{"ok":true}}"#);
            Ok(())
        }
        "drop-collection" => {
            if args.len() < 4 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            db.drop_collection(&args[3]).await?;
            println!(r#"{{"ok":true}}"#);
            Ok(())
        }
        "flush" => {
            if args.len() < 3 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            db.flush().await?;
            println!(r#"{{"ok":true}}"#);
            Ok(())
        }
        "checkpoint" => {
            if args.len() < 3 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            db.checkpoint().await?;
            println!(r#"{{"ok":true,"checkpoint":"done"}}"#);
            Ok(())
        }
        "import" | "import-json" => {
            if args.len() < 5 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            let count = db.import_json(&args[3], &args[4]).await?;
            println!(r#"{{"ok":true,"imported":{}}}"#, count);
            Ok(())
        }
        "export" | "export-json" => {
            if args.len() < 5 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            db.export_json(&args[3], &args[4]).await?;
            println!(r#"{{"ok":true,"exported_to":"{}"}}"#, args[4]);
            Ok(())
        }
        "import-csv" => {
            if args.len() < 5 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            let count = db.import_csv(&args[3], &args[4]).await?;
            println!(r#"{{"ok":true,"imported":{}}}"#, count);
            Ok(())
        }
        "export-csv" => {
            if args.len() < 5 { usage(); }
            let db = NexDb::open(&args[2]).await?;
            db.export_csv(&args[3], &args[4]).await?;
            println!(r#"{{"ok":true,"exported_to":"{}"}}"#, args[4]);
            Ok(())
        }
        "completions" => {
            if args.len() < 3 { usage(); }
            generate_completions(&args[2])
        }
        _ => usage(),
    }
}

fn generate_completions(_shell: &str) -> NexDbResult<()> {
    #[cfg(feature = "completions")]
    {
        use clap::CommandFactory;
        use clap_complete::{Generator, Shell};
        use std::io;

        let shell = match shell {
            "bash" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            "powershell" => Shell::PowerShell,
            "elvish" => Shell::Elvish,
            _ => {
                eprintln!("Unknown shell: {}. Supported: bash, zsh, fish, powershell, elvish", shell);
                std::process::exit(1);
            }
        };

        // Build a minimal clap command for completion generation
        let cmd = build_clap_app();
        let mut out = io::stdout();
        shell.generate(&cmd, &mut out);
        Ok(())
    }

    #[cfg(not(feature = "completions"))]
    {
        eprintln!("Completions not available. Rebuild with --features completions");
        std::process::exit(1);
    }
}

#[cfg(feature = "completions")]
fn build_clap_app() -> clap::Command {
    use clap::{Arg, Command};

    Command::new("nexdb")
        .about("NexDb - Embeddable Document Database")
        .subcommand(Command::new("repl").about("Start interactive REPL")
            .arg(Arg::new("db").required(true)))
        .subcommand(Command::new("serve").about("Start TCP server")
            .arg(Arg::new("db").required(true))
            .arg(Arg::new("--port").short('p').help("Port to bind")))
        .subcommand(Command::new("insert").about("Insert a document")
            .arg(Arg::new("db").required(true))
            .arg(Arg::new("collection").required(true))
            .arg(Arg::new("id").required(true))
            .arg(Arg::new("json").required(true)))
        .subcommand(Command::new("get").about("Get a document")
            .arg(Arg::new("db").required(true))
            .arg(Arg::new("collection").required(true))
            .arg(Arg::new("id").required(true)))
        .subcommand(Command::new("update").about("Update a document")
            .arg(Arg::new("db").required(true))
            .arg(Arg::new("collection").required(true))
            .arg(Arg::new("id").required(true))
            .arg(Arg::new("json").required(true)))
        .subcommand(Command::new("delete").about("Delete a document")
            .arg(Arg::new("db").required(true))
            .arg(Arg::new("collection").required(true))
            .arg(Arg::new("id").required(true)))
        .subcommand(Command::new("checkpoint").about("WAL checkpoint")
            .arg(Arg::new("db").required(true)))
        .subcommand(Command::new("import").about("Import JSON")
            .arg(Arg::new("db").required(true))
            .arg(Arg::new("collection").required(true))
            .arg(Arg::new("file").required(true)))
        .subcommand(Command::new("export").about("Export JSON")
            .arg(Arg::new("db").required(true))
            .arg(Arg::new("collection").required(true))
            .arg(Arg::new("file").required(true)))
        .subcommand(Command::new("completions").about("Generate shell completions")
            .arg(Arg::new("shell").required(true)))
}

async fn run_repl(db_path: &Path) -> NexDbResult<()> {
    let db = NexDb::open(db_path).await?;
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();
    let mut stdout = tokio::io::stdout();

    loop {
        let line = match lines.next_line().await? {
            Some(l) => l,
            None => break,
        };

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        let response = handle_json_command(&db, &trimmed).await;
        let out = serde_json::to_string(&response).unwrap_or_else(|_| {
            r#"{"ok":false,"error":"serialization error"}"#.to_string()
        });
        stdout.write_all(out.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

async fn handle_json_command(db: &NexDb, json: &str) -> Value {
    let cmd: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({"ok": false, "error": format!("invalid JSON: {}", e)}),
    };

    let command = match cmd.get("cmd").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return serde_json::json!({"ok": false, "error": "missing 'cmd' field"}),
    };

    let args = cmd.get("args").cloned().unwrap_or(Value::Null);

    match command {
        "exit" | "quit" => std::process::exit(0),

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
        _ => serde_json::json!({"ok": false, "error": format!("unknown command: {}", command)}),
    }
}

fn exec(result: NexDbResult<()>) -> Value {
    match result {
        Ok(()) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}
