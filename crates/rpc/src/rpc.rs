use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::Client;
use serde::Serialize;

use crate::Error;

#[derive(Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: serde_json::Value,
    id: u64,
}

#[derive(serde::Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// Internal JSON-RPC 2.0 transport over HTTP.
pub struct RpcTransport {
    client: Client,
    url: String,
    timeout: Duration,
    next_id: AtomicU64,
}

impl RpcTransport {
    /// Get the URL this transport connects to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Get the timeout duration for this transport.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn new(url: String, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            url,
            timeout,
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

        let body = self.send_json(&request).await?;
        let error: Option<RpcError> = body
            .get("error")
            .filter(|value| !value.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;

        if let Some(err) = error {
            return Err(Error::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        let result = body.get("result").cloned().ok_or_else(|| {
            Error::InvalidData(format!(
                "JSON-RPC response for method '{method}' missing both result and error"
            ))
        })?;

        serde_json::from_value(result).map_err(Into::into)
    }

    /// Call a JSON-RPC method that may legitimately return no value.
    ///
    /// Treats both `"result": null` and a missing `result` field as `Ok(None)`,
    /// while still propagating JSON-RPC errors.
    pub async fn call_optional<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<Option<T>, Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = Request {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };

        let body = self.send_json(&request).await?;
        let error: Option<RpcError> = body
            .get("error")
            .filter(|value| !value.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;

        if let Some(err) = error {
            return Err(Error::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        let Some(result) = body.get("result").cloned() else {
            return Ok(None);
        };

        if result.is_null() {
            return Ok(None);
        }

        serde_json::from_value(result).map(Some).map_err(Into::into)
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

        let body = self.send_json(&request).await?;
        let error: Option<RpcError> = body
            .get("error")
            .filter(|value| !value.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;

        if let Some(err) = error {
            return Err(Error::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        Ok(())
    }

    async fn send_json(&self, request: &Request<'_>) -> Result<serde_json::Value, Error> {
        let response = match self.client.post(&self.url).json(request).send().await {
            Ok(response) => response,
            Err(first_err) if first_err.is_connect() || first_err.is_request() => self
                .client
                .post(&self.url)
                .json(request)
                .send()
                .await
                .map_err(|second_err| {
                    Error::Transport(format!(
                        "{second_err}; retry after transport failure also failed: {first_err}"
                    ))
                })?,
            Err(err) => return Err(err.into()),
        };

        response.json().await.map_err(Into::into)
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
        let body: serde_json::Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","result":42,"id":1}"#).unwrap();
        assert_eq!(body["result"], serde_json::json!(42));
        assert!(body.get("error").is_none());
    }

    #[test]
    fn response_with_error_parses() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid Request"},"id":1}"#,
        )
        .unwrap();
        assert!(body.get("result").is_none());
        let err: RpcError = serde_json::from_value(body["error"].clone()).unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
    }

    #[test]
    fn response_with_null_result_parses() {
        let body: serde_json::Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","result":null,"id":1}"#).unwrap();
        assert!(body.get("result").is_some());
        assert!(body["result"].is_null());
        assert!(body.get("error").is_none());
    }
}
