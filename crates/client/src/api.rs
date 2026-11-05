//! HTTP client for the supervisor: every request carries the `x-harness-token` header.

use crate::supervisor::Info;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// HTTP status, or 0 when the supervisor could not be reached.
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.status)
    }
}

#[derive(Debug, Clone)]
pub struct Api {
    pub base: String,
    pub token: String,
}

impl Api {
    pub fn new(info: &Info) -> Api {
        Api { base: format!("http://127.0.0.1:{}", info.port), token: info.token.clone() }
    }

    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        let req = ureq::request(method, &format!("{}{path}", self.base)).set("x-harness-token", &self.token);
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match res {
            Ok(r) => {
                let text = r.into_string().map_err(|e| ApiError { status: 0, message: e.to_string() })?;
                Ok(if text.is_empty() { Value::Object(Default::default()) } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) })
            }
            Err(ureq::Error::Status(status, r)) => {
                let text = r.into_string().unwrap_or_default();
                let message = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["err"].as_str().map(str::to_string)).unwrap_or(text);
                Err(ApiError { status, message })
            }
            Err(e) => Err(ApiError { status: 0, message: e.to_string() }),
        }
    }

    pub fn get(&self, path: &str) -> Result<Value, ApiError> {
        self.request("GET", path, None)
    }
    pub fn post(&self, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        self.request("POST", path, Some(body.unwrap_or(&Value::Object(Default::default()))))
    }
    pub fn patch(&self, path: &str, body: &Value) -> Result<Value, ApiError> {
        self.request("PATCH", path, Some(body))
    }
    pub fn del(&self, path: &str) -> Result<Value, ApiError> {
        self.request("DELETE", path, None)
    }
}
