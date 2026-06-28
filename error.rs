use thiserror::Error;

#[derive(Error, Debug)]
pub enum NexDbError {
    #[error("document not found: {0}")]
    NotFound(String),

    #[error("document already exists: {0}")]
    Duplicate(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("collection '{0}' not found")]
    CollectionNotFound(String),

    #[error("collection already exists: {0}")]
    CollectionAlreadyExists(String),

    #[error("WAL error: {0}")]
    Wal(String),

    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("index error: {0}")]
    Index(String),
}

pub type NexDbResult<T> = Result<T, NexDbError>;
