use thiserror::Error;

#[derive(Debug, Error)]
pub enum AssuranceError {
    #[error("entity resolution failed: {0}")]
    Resolution(String),
    #[error("retrieval failed: {0}")]
    Retrieval(String),
    #[error("attestation signing failed: {0}")]
    Attestation(String),
    #[error("backend error: {0}")]
    Backend(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, AssuranceError>;
