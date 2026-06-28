pub mod collection;
pub mod db;
pub mod document;
pub mod error;
pub mod index;
pub mod query;
pub mod server;
pub mod wal;

pub use collection::Collection;
pub use db::NexDb;
pub use document::Document;
pub use error::{NexDbError, NexDbResult};
pub use index::FieldIndex;
pub use wal::{WalEntry, WalOperation, WalReader, WalWriter};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
