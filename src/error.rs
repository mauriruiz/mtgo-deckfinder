//! Library error type. The binary boundary wraps these with `anyhow`.

use thiserror::Error;

/// Errors raised by the library's impure edges (store IO, (de)serialization).
#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Convenience alias for results carrying the library [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
