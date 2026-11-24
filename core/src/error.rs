use thiserror::Error;

/// Every failure the core can report to a front-end.
///
/// `Validation` and `Auth` messages are written for end users and can be shown
/// as-is; the other variants wrap lower-level failures.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Auth(String),

    #[error("Too many failed attempts. Try again in {0} seconds.")]
    LockedOut(i64),

    #[error("{0} not found")]
    NotFound(&'static str),

    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("File error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid template XML: {0}")]
    Xml(String),

    #[error("Data error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Backup archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("PDF error: {0}")]
    Pdf(String),

    #[error("Security error: {0}")]
    Crypto(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

pub(crate) fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(CoreError::Validation(msg.into()))
}
