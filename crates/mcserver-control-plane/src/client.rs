use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use mcserver_protocol::json_rpc;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

#[derive(Debug, Clone)]
pub struct UnixRpcClient {
    socket_path: PathBuf,
    max_frame_bytes: usize,
    timeout: Duration,
}

impl UnixRpcClient {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            max_frame_bytes: 1024 * 1024,
            timeout: Duration::from_secs(30),
        }
    }

    #[must_use]
    pub const fn with_max_frame_bytes(mut self, max_frame_bytes: usize) -> Self {
        self.max_frame_bytes = max_frame_bytes;
        self
    }

    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub async fn call<P, R>(&self, method: &str, params: &P) -> Result<R, RpcClientError>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let params = serde_json::to_value(params)?;
        let result = self.call_value(method, params).await?;
        serde_json::from_value(result).map_err(RpcClientError::Serialization)
    }

    pub async fn call_value(&self, method: &str, params: Value) -> Result<Value, RpcClientError> {
        let request = json!({
            "jsonrpc": json_rpc::VERSION,
            "method": method,
            "params": params,
            "id": 1,
        });
        let encoded = serde_json::to_vec(&request)?;
        if encoded.len().saturating_add(1) > self.max_frame_bytes {
            return Err(RpcClientError::FrameTooLarge {
                actual: encoded.len().saturating_add(1),
                maximum: self.max_frame_bytes,
            });
        }

        let stream = tokio::time::timeout(self.timeout, UnixStream::connect(&self.socket_path))
            .await
            .map_err(|_| RpcClientError::Timeout("connecting to control-plane"))??;
        let (reader, mut writer) = stream.into_split();
        writer.write_all(&encoded).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        let read_limit = u64::try_from(self.max_frame_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut reader = BufReader::new(reader.take(read_limit));
        let mut response = Vec::new();
        let read = tokio::time::timeout(
            self.timeout,
            reader.read_until(b'\n', &mut response),
        )
        .await
        .map_err(|_| RpcClientError::Timeout("waiting for control-plane response"))??;
        if read == 0 {
            return Err(RpcClientError::Disconnected);
        }
        if response.len() > self.max_frame_bytes {
            return Err(RpcClientError::FrameTooLarge {
                actual: response.len(),
                maximum: self.max_frame_bytes,
            });
        }
        if response.last() != Some(&b'\n') {
            return Err(RpcClientError::InvalidResponse(
                "response is not newline-terminated".to_owned(),
            ));
        }
        while response
            .last()
            .is_some_and(|byte| matches!(*byte, b'\n' | b'\r'))
        {
            response.pop();
        }

        let response = serde_json::from_slice::<Value>(&response)?;
        if response.get("jsonrpc").and_then(Value::as_str) != Some(json_rpc::VERSION) {
            return Err(RpcClientError::InvalidResponse(
                "JSON-RPC version is missing or unsupported".to_owned(),
            ));
        }
        if response.get("id") != Some(&Value::from(1)) {
            return Err(RpcClientError::InvalidResponse(
                "JSON-RPC response id does not match the request".to_owned(),
            ));
        }
        if let Some(error) = response.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or_default();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown JSON-RPC error")
                .to_owned();
            return Err(RpcClientError::Remote {
                code,
                message,
                data: error.get("data").cloned(),
            });
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| RpcClientError::InvalidResponse("response has no result".to_owned()))
    }
}

#[derive(Debug, Error)]
pub enum RpcClientError {
    #[error("control-plane client I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("control-plane JSON serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("control-plane operation timed out while {0}")]
    Timeout(&'static str),
    #[error("control-plane disconnected before returning a response")]
    Disconnected,
    #[error("control-plane frame is too large: {actual} bytes, maximum {maximum}")]
    FrameTooLarge { actual: usize, maximum: usize },
    #[error("invalid control-plane response: {0}")]
    InvalidResponse(String),
    #[error("control-plane returned JSON-RPC error {code}: {message}")]
    Remote {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

#[cfg(test)]
mod tests {
    use std::{error::Error, path::PathBuf, time::Duration};

    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };
    use uuid::Uuid;

    use super::UnixRpcClient;

    #[tokio::test]
    async fn sends_one_framed_request_and_decodes_the_result(
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let socket_path = temporary_socket_path();
        let listener = UnixListener::bind(&socket_path)?;
        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (stream, _) = listener.accept().await?;
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let mut request = String::new();
                reader.read_line(&mut request).await?;
                let request = serde_json::from_str::<Value>(&request)?;
                assert_eq!(request["method"], "test.echo");
                assert_eq!(request["params"], json!({ "value": 7 }));
                writer
                    .write_all(
                        b"{\"jsonrpc\":\"2.0\",\"result\":{\"value\":7},\"id\":1}\n",
                    )
                    .await?;
                writer.flush().await?;
                tokio::fs::remove_file(socket_path).await?;
                Ok::<(), Box<dyn Error + Send + Sync>>(())
            }
        });

        let result = UnixRpcClient::new(&socket_path)
            .with_timeout(Duration::from_secs(2))
            .call_value("test.echo", json!({ "value": 7 }))
            .await?;
        assert_eq!(result, json!({ "value": 7 }));
        server.await??;
        Ok(())
    }

    fn temporary_socket_path() -> PathBuf {
        std::env::temp_dir().join(format!("mcserver-client-test-{}.sock", Uuid::new_v4()))
    }
}
