use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::Error;

#[derive(Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: serde_json::Value,
    id: u64,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<RpcError>,
    #[allow(dead_code)]
    id: u64,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// Internal JSON-RPC 2.0 transport over HTTP.
pub struct RpcTransport {
    client: Client,
    url: String,
    next_id: AtomicU64,
}

impl RpcTransport {
    pub fn new(url: String, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            url,
            next_id: AtomicU64::new(1),
        }
    }

    pub async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = Request {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };

        let resp = self.client.post(&self.url).json(&request).send().await?;
        let body: Response = resp.json().await?;

        if let Some(err) = body.error {
            return Err(Error::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        let result = body.result.ok_or_else(|| {
            Error::InvalidData("JSON-RPC response missing both result and error".into())
        })?;

        serde_json::from_value(result).map_err(Into::into)
    }

    /// Call a JSON-RPC method that returns no meaningful value.
    ///
    /// A null or missing result is accepted without error. Only JSON-RPC
    /// error responses are propagated.
    pub async fn call_void(&self, method: &str, params: serde_json::Value) -> Result<(), Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = Request {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };

        let resp = self.client.post(&self.url).json(&request).send().await?;
        let body: Response = resp.json().await?;

        if let Some(err) = body.error {
            return Err(Error::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn request_envelope_serializes() {
        let req = Request {
            jsonrpc: "2.0",
            method: "test_method",
            params: serde_json::json!([1, "two"]),
            id: 42,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "test_method");
        assert_eq!(json["id"], 42);
        assert_eq!(json["params"][0], 1);
        assert_eq!(json["params"][1], "two");
    }

    #[test]
    fn response_with_result_parses() {
        let json = r#"{"jsonrpc":"2.0","result":42,"id":1}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp.result.unwrap(), serde_json::json!(42));
        assert!(resp.error.is_none());
    }

    #[test]
    fn response_with_error_parses() {
        let json =
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid Request"},"id":1}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
    }

    #[test]
    fn response_with_null_result_parses() {
        let json = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        // Option deserializes JSON null as None
        assert!(resp.result.is_none());
        assert!(resp.error.is_none());
    }
}
