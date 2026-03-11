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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ensure_object_schema ──────────────────────────────────────────

    #[test]
    fn ensure_object_schema_adds_type_when_missing() {
        let schema = json!({ "properties": { "a": { "type": "string" } } });
        let result = ensure_object_schema(&schema);
        assert_eq!(result["type"], "object");
        // Original properties preserved
        assert_eq!(result["properties"]["a"]["type"], "string");
    }

    #[test]
    fn ensure_object_schema_preserves_existing_type() {
        let schema = json!({ "type": "array", "items": { "type": "string" } });
        let result = ensure_object_schema(&schema);
        assert_eq!(result["type"], "array");
    }

    #[test]
    fn ensure_object_schema_handles_non_object() {
        let schema = json!("just a string");
        let result = ensure_object_schema(&schema);
        assert_eq!(result, json!("just a string"));
    }

    #[test]
    fn ensure_object_schema_handles_empty_object() {
        let schema = json!({});
        let result = ensure_object_schema(&schema);
        assert_eq!(result, json!({ "type": "object" }));
    }

    // ── strip_schema_keys ─────────────────────────────────────────────

    #[test]
    fn strip_schema_keys_removes_specified_keys() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "minLength": 1, "maxLength": 100 }
            },
            "minItems": 1
        });
        let result = strip_schema_keys(&schema, &["minLength", "maxLength", "minItems"]);
        // Top-level minItems removed
        assert!(result.get("minItems").is_none());
        // Nested minLength/maxLength removed
        assert!(result["properties"]["name"].get("minLength").is_none());
        assert!(result["properties"]["name"].get("maxLength").is_none());
        // type preserved
        assert_eq!(result["properties"]["name"]["type"], "string");
    }

    #[test]
    fn strip_schema_keys_preserves_properties_field_names() {
        // If a property is named "pattern" or "format", it should NOT be stripped
        // because inside "properties" block, keys are field names, not schema keywords.
        let schema = json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "pattern": "^[a-z]+$" },
                "format": { "type": "string" }
            }
        });
        let result = strip_schema_keys(&schema, &["pattern", "format"]);
        // The field names "pattern" and "format" inside properties must survive
        assert!(result["properties"].get("pattern").is_some());
        assert!(result["properties"].get("format").is_some());
        // But the "pattern" keyword inside the "pattern" property's schema is stripped
        assert!(result["properties"]["pattern"].get("pattern").is_none());
    }

    #[test]
    fn strip_schema_keys_handles_nested_objects() {
        let schema = json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object",
                    "properties": {
                        "val": { "type": "integer", "minimum": 0, "maximum": 100 }
                    }
                }
            }
        });
        let result = strip_schema_keys(&schema, &["minimum", "maximum"]);
        assert!(
            result["properties"]["nested"]["properties"]["val"]
                .get("minimum")
                .is_none()
        );
        assert!(
            result["properties"]["nested"]["properties"]["val"]
                .get("maximum")
                .is_none()
        );
        assert_eq!(
            result["properties"]["nested"]["properties"]["val"]["type"],
            "integer"
        );
    }

    #[test]
    fn strip_schema_keys_handles_arrays() {
        let schema = json!({
            "type": "array",
            "items": { "type": "string", "minLength": 1 }
        });
        let result = strip_schema_keys(&schema, &["minLength"]);
        assert!(result["items"].get("minLength").is_none());
        assert_eq!(result["items"]["type"], "string");
    }

    #[test]
    fn strip_schema_keys_no_keys_to_remove() {
        let schema = json!({ "type": "string", "minLength": 1 });
        let result = strip_schema_keys(&schema, &[]);
        assert_eq!(result, schema);
    }

    #[test]
    fn strip_schema_keys_scalar_unchanged() {
        let schema = json!(42);
        let result = strip_schema_keys(&schema, &["anything"]);
        assert_eq!(result, json!(42));
    }
}
