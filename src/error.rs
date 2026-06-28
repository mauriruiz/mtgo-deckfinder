//! Library error type. The binary boundary wraps these with `anyhow`.

use thiserror::Error;

/// Errors raised by the library's impure edges (network, store IO,
/// (de)serialization, parsing).
#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("checksum mismatch for {0}")]
    Checksum(String),
    #[error("parse error: {0}")]
    Parse(String),
}

/// Convenience alias for results carrying the library [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
