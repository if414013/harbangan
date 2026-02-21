//! Tool schema sanitization for Google protobuf format.
//!
//! Google's Generative AI API uses a subset of JSON Schema.
//! This module strips unsupported fields that would cause protobuf
//! deserialization errors.

use serde_json::Value;

/// JSON Schema fields not supported by Google's protobuf format.
const UNSUPPORTED_FIELDS: &[&str] = &[
    "$schema",
    "additionalProperties",
    "default",
    "examples",
    "id",
    "$id",
    "$ref",
    "definitions",
    "$defs",
    "patternProperties",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "dependentSchemas",
    "dependentRequired",
    "title",
    "readOnly",
    "writeOnly",
    "deprecated",
    "const",
    "contentMediaType",
    "contentEncoding",
    "minContains",
    "maxContains",
    "prefixItems",
    "unevaluatedItems",
    "unevaluatedProperties",
];

/// Recursively sanitizes a JSON Schema value for Google protobuf compatibility.
///
/// Removes unsupported fields and recurses into `properties`, `items`,
/// and nested schemas.
pub fn sanitize_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut result = serde_json::Map::new();

            for (key, value) in map {
                // Skip unsupported fields
                if UNSUPPORTED_FIELDS.contains(&key.as_str()) {
                    continue;
                }

                // Recurse into nested schemas
                let sanitized = match key.as_str() {
                    "properties" => sanitize_properties(value),
                    "items" => sanitize_schema(value),
                    "required" => value.clone(), // pass through arrays as-is
                    _ => sanitize_schema(value),
                };

                result.insert(key.clone(), sanitized);
            }

            Value::Object(result)
        }
        // Non-object values pass through unchanged
        other => other.clone(),
    }
}

/// Sanitizes each property in a `properties` object.
fn sanitize_properties(properties: &Value) -> Value {
    match properties {
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                result.insert(key.clone(), sanitize_schema(value));
            }
            Value::Object(result)
        }
        other => other.clone(),
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_sanitize_removes_unsupported_fields() {
        let schema = json!({
            "type": "object",
            "$schema": "http://json-schema.org/draft-07/schema#",
            "additionalProperties": false,
            "default": {},
            "title": "MySchema",
            "properties": {
                "name": {
                    "type": "string",
                    "default": "hello",
                    "description": "The name"
                }
            },
            "required": ["name"]
        });

        let sanitized = sanitize_schema(&schema);
        let obj = sanitized.as_object().unwrap();

        assert!(obj.contains_key("type"));
        assert!(obj.contains_key("properties"));
        assert!(obj.contains_key("required"));
        assert!(!obj.contains_key("$schema"));
        assert!(!obj.contains_key("additionalProperties"));
        assert!(!obj.contains_key("default"));
        assert!(!obj.contains_key("title"));

        // Nested property should also be sanitized
        let props = obj["properties"].as_object().unwrap();
        let name_prop = props["name"].as_object().unwrap();
        assert!(name_prop.contains_key("type"));
        assert!(name_prop.contains_key("description"));
        assert!(!name_prop.contains_key("default"));
    }

    #[test]
    fn test_sanitize_preserves_supported_fields() {
        let schema = json!({
            "type": "object",
            "description": "A test schema",
            "properties": {
                "count": {
                    "type": "integer",
                    "description": "A count",
                    "minimum": 0,
                    "maximum": 100
                }
            },
            "required": ["count"]
        });

        let sanitized = sanitize_schema(&schema);
        let obj = sanitized.as_object().unwrap();

        assert_eq!(obj["type"], "object");
        assert_eq!(obj["description"], "A test schema");
        assert_eq!(obj["required"], json!(["count"]));
    }

    #[test]
    fn test_sanitize_nested_items() {
        let schema = json!({
            "type": "array",
            "items": {
                "type": "string",
                "default": "foo",
                "title": "Item"
            }
        });

        let sanitized = sanitize_schema(&schema);
        let items = sanitized["items"].as_object().unwrap();
        assert!(items.contains_key("type"));
        assert!(!items.contains_key("default"));
        assert!(!items.contains_key("title"));
    }

    #[test]
    fn test_sanitize_non_object() {
        assert_eq!(sanitize_schema(&json!("string")), json!("string"));
        assert_eq!(sanitize_schema(&json!(42)), json!(42));
        assert_eq!(sanitize_schema(&json!(true)), json!(true));
        assert_eq!(sanitize_schema(&json!(null)), json!(null));
    }

    #[test]
    fn test_sanitize_empty_object() {
        let sanitized = sanitize_schema(&json!({}));
        assert_eq!(sanitized, json!({}));
    }
}
