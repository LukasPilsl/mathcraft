//! JSON-Schema builders for tool inputs, and a typed reader for the arguments of a call. Every
//! tool group builds its `inputSchema` with these helpers so all schemas look alike.

use serde_json::{json, Map, Value};

use crate::error::ToolError;

/// `{"type":"string","description":...}`.
pub fn string(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

pub fn integer(description: &str) -> Value {
    json!({"type": "integer", "minimum": 0, "description": description})
}

pub fn boolean(description: &str) -> Value {
    json!({"type": "boolean", "description": description})
}

/// An object schema that refuses unknown properties.
pub fn object(properties: &[(&str, Value)], required: &[&str]) -> Value {
    let props: Map<String, Value> = properties
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

/// The schema of a tool without arguments.
pub fn empty() -> Value {
    object(&[], &[])
}

/// The `document_id` property every document tool takes.
pub fn document_id() -> (&'static str, Value) {
    (
        "document_id",
        string("Id returned by document_new or document_open"),
    )
}

/// The arguments of one call, read with typed accessors that fail as `invalid_params`.
pub struct Args<'a>(pub &'a Map<String, Value>);

impl Args<'_> {
    pub fn str(&self, key: &str) -> Result<&str, ToolError> {
        self.opt_str(key)?
            .ok_or_else(|| ToolError::InvalidParams(format!("missing argument `{key}`")))
    }

    pub fn opt_str(&self, key: &str) -> Result<Option<&str>, ToolError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(ToolError::InvalidParams(format!(
                "`{key}` must be a string"
            ))),
        }
    }

    pub fn opt_bool(&self, key: &str) -> Result<bool, ToolError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(ToolError::InvalidParams(format!(
                "`{key}` must be a boolean"
            ))),
        }
    }

    pub fn opt_u64(&self, key: &str) -> Result<Option<u64>, ToolError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| {
                ToolError::InvalidParams(format!("`{key}` must be a non-negative integer"))
            }),
        }
    }

    pub fn u64(&self, key: &str) -> Result<u64, ToolError> {
        self.opt_u64(key)?
            .ok_or_else(|| ToolError::InvalidParams(format!("missing argument `{key}`")))
    }

    /// A region id: a non-negative integer that fits `u32`.
    pub fn region_id(&self, key: &str) -> Result<u32, ToolError> {
        u32::try_from(self.u64(key)?)
            .map_err(|_| ToolError::InvalidParams(format!("`{key}` is not a region id")))
    }

    /// Rejects properties the schema does not list, so a typo is an error, not a silent no-op.
    pub fn only(&self, allowed: &[&str]) -> Result<(), ToolError> {
        match self.0.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(k) => Err(ToolError::InvalidParams(format!("unknown argument `{k}`"))),
            None => Ok(()),
        }
    }
}
