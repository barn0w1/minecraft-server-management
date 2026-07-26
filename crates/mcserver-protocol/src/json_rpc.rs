use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const VERSION: &str = "2.0";

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default, deserialize_with = "deserialize_request_id")]
    pub id: RequestId,
}

#[derive(Debug, Clone, Default)]
pub enum RequestId {
    #[default]
    Missing,
    Present(Value),
}

impl RequestId {
    #[must_use]
    pub fn is_notification(&self) -> bool {
        matches!(self, Self::Missing)
    }

    #[must_use]
    pub fn response_id(&self) -> Option<Value> {
        match self {
            Self::Missing => None,
            Self::Present(value) => Some(value.clone()),
        }
    }
}

fn deserialize_request_id<'de, D>(deserializer: D) -> Result<RequestId, D::Error>
where
    D: Deserializer<'de>,
{
    Value::deserialize(deserializer).map(RequestId::Present)
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
    pub id: Value,
}

impl Response {
    #[must_use]
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: VERSION,
            result: Some(result),
            error: None,
            id,
        }
    }

    #[must_use]
    pub fn error(id: Value, error: ErrorObject) -> Self {
        Self {
            jsonrpc: VERSION,
            result: None,
            error: Some(error),
            id,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorObject {
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

pub mod error_code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    pub const CONFLICT: i64 = -32001;
    pub const NOT_FOUND: i64 = -32004;
}
