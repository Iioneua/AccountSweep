//! Error type for the core library.

use crate::output::ErrorCode;

/// Result alias used across the core library.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by core operations. Each maps to a stable [`ErrorCode`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error with a stable code and a message safe to show to users and agents.
    #[error("{message}")]
    Coded {
        /// Stable error code.
        code: ErrorCode,
        /// Explanation. Never contains secrets or the user's personal data.
        message: String,
    },
    /// SQLite failure.
    #[error("local database error: {0}")]
    Storage(#[from] rusqlite::Error),
    /// File-system failure.
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Build a coded error.
    pub fn coded(code: ErrorCode, message: impl Into<String>) -> Self {
        Error::Coded {
            code,
            message: message.into(),
        }
    }

    /// The stable code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::Coded { code, .. } => *code,
            Error::Storage(_) => ErrorCode::Storage,
            Error::Io(_) => ErrorCode::Internal,
        }
    }
}
