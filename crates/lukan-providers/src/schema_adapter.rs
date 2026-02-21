/// Adapts tool input schemas between different provider formats.
///
/// Our canonical format uses JSON Schema. Different providers may need
/// slight adjustments (e.g., Anthropic requires `type: "object"` at top level).
use serde_json::Value;

/// Ensure the schema has `type: "object"` at the top level (Anthropic requirement)
pub fn ensure_object_schema(schema: &Value) -> Value {
    let mut schema = schema.clone();
    if let Some(obj) = schema.as_object_mut()
        && !obj.contains_key("type")
    {
        obj.insert("type".to_string(), Value::String("object".to_string()));
    }
    schema
}

/// Strip unsupported keys from a schema for a given provider
pub fn strip_schema_keys(schema: &Value, keys_to_remove: &[&str]) -> Value {
    match schema {
        Value::Object(obj) => {
            let mut new_obj = serde_json::Map::new();
            for (k, v) in obj {
                if !keys_to_remove.contains(&k.as_str()) {
                    new_obj.insert(k.clone(), strip_schema_keys(v, keys_to_remove));
                }
            }
            Value::Object(new_obj)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| strip_schema_keys(v, keys_to_remove))
                .collect(),
        ),
        other => other.clone(),
    }
}
