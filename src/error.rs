use thiserror::Error;

/// Crate-level error type for aztec-rs.
#[derive(Debug, Error)]
pub enum Error {
    /// HTTP or transport-layer failure.
    #[error("transport error: {0}")]
    Transport(String),

    /// JSON serialization/deserialization failure.
    #[error("json error: {0}")]
    Json(String),

    /// ABI or data validation failure.
    #[error("abi error: {0}")]
    Abi(String),

    /// Invalid or unexpected data.
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// JSON-RPC error returned by the server.
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },

    /// Transaction execution reverted.
    #[error("reverted: {0}")]
    Reverted(String),

    /// Operation timed out.
    #[error("timeout: {0}")]
    Timeout(String),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::Transport(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Self::Transport(e.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn transport_error_display() {
        let err = Error::Transport("connection refused".into());
        assert_eq!(err.to_string(), "transport error: connection refused");
    }

    #[test]
    fn json_error_display() {
        let err = Error::Json("unexpected token".into());
        assert_eq!(err.to_string(), "json error: unexpected token");
    }

    #[test]
    fn abi_error_display() {
        let err = Error::Abi("unknown type".into());
        assert_eq!(err.to_string(), "abi error: unknown type");
    }

    #[test]
    fn invalid_data_error_display() {
        let err = Error::InvalidData("field out of range".into());
        assert_eq!(err.to_string(), "invalid data: field out of range");
    }

    #[test]
    fn rpc_error_display() {
        let err = Error::Rpc {
            code: -32600,
            message: "invalid request".into(),
        };
        assert_eq!(err.to_string(), "rpc error -32600: invalid request");
    }

    #[test]
    fn reverted_error_display() {
        let err = Error::Reverted("assertion failed".into());
        assert_eq!(err.to_string(), "reverted: assertion failed");
    }

    #[test]
    fn timeout_error_display() {
        let err = Error::Timeout("waited 30s".into());
        assert_eq!(err.to_string(), "timeout: waited 30s");
    }

    #[test]
    fn from_serde_json_error() {
        let raw = serde_json::from_str::<serde_json::Value>("not json");
        let serde_err = raw.unwrap_err();
        let err: Error = serde_err.into();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn from_url_parse_error() {
        let url_err = url::Url::parse("not a url :://").unwrap_err();
        let err: Error = url_err.into();
        assert!(matches!(err, Error::Transport(_)));
    }

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }
}
