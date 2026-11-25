//! The input schema a tool advertises, derived from its input type.

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value};

/// The JSON Schema (draft 2020-12) of what `I` accepts when parsed, as the
/// protocol wants it: an object schema, without the type's own title and
/// description, which name the Rust type rather than the tool.
///
/// The reason it is refused when it is not an object schema.
pub(super) fn input_schema<I: JsonSchema>() -> Result<Map<String, Value>, String> {
    let schema = SchemaSettings::draft2020_12()
        .for_deserialize()
        .into_generator()
        .into_root_schema_for::<I>();
    let mut object = match serde_json::to_value(schema) {
        Ok(Value::Object(object)) => object,
        Ok(other) => return Err(format!("the schema is not a JSON object but {other}")),
        Err(error) => return Err(format!("the schema cannot be encoded: {error}")),
    };
    match object.get("type") {
        Some(Value::String(kind)) if kind == "object" => {}
        Some(kind) => return Err(format!("its root type is {kind}, not \"object\"")),
        None => return Err("its root has no type, not \"object\"".to_owned()),
    }
    object.remove("title");
    object.remove("description");
    Ok(object)
}
