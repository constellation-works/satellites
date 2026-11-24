//! A tool's JSON as structured content.

use serde_json::{Value, json};

/// The structured content for a tool's JSON: an object as it is, a list as
/// `{"items": [...]}`, and any other value as `{"value": ...}`.
///
/// ```
/// use serde_json::json;
///
/// assert_eq!(cw_mcp::payload::structured(json!({"id": 1})), json!({"id": 1}));
/// assert_eq!(cw_mcp::payload::structured(json!([1, 2])), json!({"items": [1, 2]}));
/// assert_eq!(cw_mcp::payload::structured(json!("done")), json!({"value": "done"}));
/// ```
#[must_use]
pub fn structured(value: Value) -> Value {
    match value {
        Value::Object(_) => value,
        Value::Array(items) => json!({ "items": items }),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => {
            json!({ "value": other })
        }
    }
}
