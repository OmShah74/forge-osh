//! Runtime JSON-Schema validation for tool inputs.
//!
//! Every `Tool::parameters_schema()` returns an OpenAI-style JSON schema; this
//! module compiles the schema on first use (per call site a schema is effectively
//! static, but we re-compile per call because the trait returns an owned Value
//! — cheap relative to the HTTP cost of a tool round trip) and returns a human
//! readable error string if the tool input does not match.
//!
//! Errors are deterministic: same input → same message, so the agent can reason
//! about them during retry loops.

use jsonschema::JSONSchema;
use serde_json::Value;

/// Validate `input` against `schema`. Returns `Ok(())` on success or an
/// explanatory string on failure. A schema that fails to compile returns
/// `Ok(())` — tool authors should ship valid schemas but we don't want a
/// buggy schema to permanently block the tool.
pub fn validate_input(schema: &Value, input: &Value) -> Result<(), String> {
    // An empty/missing "type" means "we don't enforce shape" — skip.
    if schema.get("type").is_none() && schema.get("properties").is_none() {
        return Ok(());
    }

    let compiled = match JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(schema)
    {
        Ok(c) => c,
        Err(_) => return Ok(()), // invalid schema → skip validation
    };

    if let Err(errors) = compiled.validate(input) {
        let mut lines = Vec::new();
        for (i, err) in errors.enumerate() {
            if i >= 5 {
                // cap at first 5 errors to keep the prompt tight
                lines.push("  (…further errors omitted)".to_string());
                break;
            }
            lines.push(format!("  - {} (at {})", err, err.instance_path));
        }
        return Err(format!("schema validation failed:\n{}", lines.join("\n")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_valid_input_passes() {
        let schema = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        });
        let input = json!({"path": "/tmp/x"});
        assert!(validate_input(&schema, &input).is_ok());
    }

    #[test]
    fn test_missing_required_fails() {
        let schema = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        });
        let input = json!({});
        assert!(validate_input(&schema, &input).is_err());
    }

    #[test]
    fn test_wrong_type_fails() {
        let schema = json!({
            "type": "object",
            "properties": { "count": { "type": "integer" } },
            "required": ["count"]
        });
        let input = json!({"count": "not a number"});
        assert!(validate_input(&schema, &input).is_err());
    }

    #[test]
    fn test_empty_schema_passes() {
        let schema = json!({});
        assert!(validate_input(&schema, &json!({})).is_ok());
    }
}
