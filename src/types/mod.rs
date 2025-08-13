use serde::{Deserialize, Serialize};
use std::fmt;

/// Newtype for request IDs to ensure type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub usize);

impl RequestId {
    /// Create a new RequestId
    pub fn new(id: usize) -> Self {
        Self(id)
    }

    /// Get the inner value
    pub fn inner(&self) -> usize {
        self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for RequestId {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<RequestId> for usize {
    fn from(value: RequestId) -> usize {
        value.0
    }
}

/// Newtype for tool names to ensure type safety
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolName(pub String);

impl ToolName {
    /// Create a new ToolName
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Get a string slice of the tool name
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the inner String
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ToolName {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ToolName {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl AsRef<str> for ToolName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
