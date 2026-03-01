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

/// Strip unsupported JSON Schema keywords from a tool schema.
///
/// `is_properties_block` tracks whether we're inside a `"properties"` object,
/// where keys are field names (e.g. `"pattern"`, `"format"`) — not schema
/// keywords — and must NOT be stripped.
pub fn strip_schema_keys(schema: &Value, keys_to_remove: &[&str]) -> Value {
    strip_schema_keys_inner(schema, keys_to_remove, false)
}

fn strip_schema_keys_inner(
    schema: &Value,
    keys_to_remove: &[&str],
    is_properties_block: bool,
) -> Value {
    match schema {
        Value::Object(obj) => {
            let mut new_obj = serde_json::Map::new();
            for (k, v) in obj {
                if !is_properties_block && keys_to_remove.contains(&k.as_str()) {
                    continue;
                }
                let child_is_props = k == "properties";
                new_obj.insert(
                    k.clone(),
                    strip_schema_keys_inner(v, keys_to_remove, child_is_props),
                );
            }
            Value::Object(new_obj)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| strip_schema_keys_inner(v, keys_to_remove, false))
                .collect(),
        ),
        other => other.clone(),
    }
}
