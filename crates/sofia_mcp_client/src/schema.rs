//! Project MCP JSON Schema into the Gemini function parameter schema subset.
use serde_json::{Map, Value};

pub fn convert(root: &Value) -> Result<Value, String> {
    visit(root, root, 0)
}
fn visit(value: &Value, root: &Value, depth: usize) -> Result<Value, String> {
    if depth > 24 {
        return Err("Recursive/deep JSON schema".into());
    }
    let object = value.as_object().ok_or("Schema must be an object")?;
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        let path = reference
            .strip_prefix('#')
            .ok_or("External schema reference")?;
        return visit(
            root.pointer(path).ok_or("Unresolved schema reference")?,
            root,
            depth + 1,
        );
    }
    let mut result = Map::new();
    for (key, value) in object {
        match key.as_str() {
            "type" => {
                if let Some(types) = value.as_array() {
                    let alternatives: Vec<Value> = types
                        .iter()
                        .filter(|kind| kind.as_str() != Some("null"))
                        .map(|kind| serde_json::json!({"type":kind}))
                        .collect();
                    if alternatives.len() == 1 {
                        result.insert("type".into(), alternatives[0]["type"].clone());
                    } else {
                        result.insert("anyOf".into(), Value::Array(alternatives));
                    }
                    if types.iter().any(|kind| kind.as_str() == Some("null")) {
                        result.insert("nullable".into(), Value::Bool(true));
                    }
                } else {
                    result.insert(key.clone(), value.clone());
                }
            }
            "properties" => {
                let properties = value
                    .as_object()
                    .ok_or("Invalid properties")?
                    .iter()
                    .map(|(name, schema)| Ok((name.clone(), visit(schema, root, depth + 1)?)))
                    .collect::<Result<Map<String, Value>, String>>()?;
                result.insert(key.clone(), Value::Object(properties));
            }
            "items" => {
                result.insert(key.clone(), visit(value, root, depth + 1)?);
            }
            "anyOf" | "oneOf" => {
                let schemas = value
                    .as_array()
                    .ok_or("Invalid alternatives")?
                    .iter()
                    .map(|schema| visit(schema, root, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                result.insert("anyOf".into(), Value::Array(schemas));
            }
            "const" => {
                result.insert("enum".into(), Value::Array(vec![value.clone()]));
            }
            "description" | "format" | "enum" | "required" | "minimum" | "maximum" | "minItems"
            | "maxItems" | "nullable" => {
                result.insert(key.clone(), value.clone());
            }
            "allOf" | "not" | "if" | "then" | "else" => {
                return Err("Unsupported schema composition".into());
            }
            _ => {}
        }
    }
    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn resolves_local_refs_and_nullable_types() {
        let schema = json!({"type":"object","properties":{"name":{"$ref":"#/$defs/name"}},"$defs":{"name":{"type":["string","null"]}}});
        let converted = convert(&schema).unwrap();
        assert_eq!(converted["properties"]["name"]["type"], "string");
        assert_eq!(converted["properties"]["name"]["nullable"], true);
        assert!(converted.get("$defs").is_none());
    }
    #[test]
    fn rejects_recursive_external_and_unsupported_composition() {
        for schema in [
            json!({"$ref":"#"}),
            json!({"$ref":"https://example.com/schema"}),
            json!({"allOf":[]}),
        ] {
            assert!(convert(&schema).is_err());
        }
    }
}
