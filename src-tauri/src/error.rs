use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, WitnessError>;

#[derive(Debug, Error)]
pub enum WitnessError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("worker queue is closed")]
    WorkerClosed,
    #[error("operation timed out: {0}")]
    Timeout(String),
    #[error("upstream refused with status {status}: {message}")]
    UpstreamRefused { status: u16, message: String },
    #[error("invalid HTTP message: {0}")]
    InvalidHttp(String),
    #[error("unsupported HTTP/2 cleartext or upgrade request")]
    Http2Unsupported,
    #[error("HTTP/2 error: {message}")]
    Http2 {
        message: String,
        client_cancelled: bool,
    },
    #[error("proxy error: {0}")]
    Proxy(String),
    #[error("TLS error: {0}")]
    Tls(String),
    #[error("project error: {0}")]
    Project(String),
    #[error("organizer error: {0}")]
    Organizer(String),
    #[error("identity error: {0}")]
    Identity(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl WitnessError {
    pub fn client_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::Http2 {
                    client_cancelled: true,
                    ..
                }
        )
    }
}

impl serde::Serialize for WitnessError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
