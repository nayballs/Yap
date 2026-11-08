//! **A connector tool's input schema, made acceptable to every provider**
//! Yap sends tools to: OpenAI, Anthropic, Gemini (the strictest: an OpenAPI
//! subset), Groq and the on-device model's server. A server's schema can be
//! any JSON Schema, so [`make_safe`] keeps the part all of them read and
//! drops the rest:
//!
//! - local `$ref`s (`#/$defs/…`, `#/definitions/…`) resolved in place, a
//!   recursive one left as a plain object past [`MAX_DEPTH`]; a remote one
//!   refused;
//! - the top `type: "object"` ensured (with `properties`), anything else
//!   refused;
//! - `type: ["string", "null"]` → `type: "string"` + `nullable`; `const` →
//!   a one-value `enum`; `oneOf` → `anyOf`; an `allOf` of objects merged;
//! - only the keywords on [`KEEP`] kept (no `$schema`, `examples`,
//!   `additionalProperties`, `patternProperties`, `if`/`then`…), `required`
//!   only of properties there are, an `enum` only of strings, an array's
//!   missing `items` given one;
//! - nesting past [`MAX_DEPTH`] made a plain object, and a schema still over
//!   [`MAX_BYTES`] refused.
//!
//! A refusal is the reason in plain words, shown on the tool's row; the
//! tool is never offered (`cache.rs`).

use serde_json::{json, Map, Value};

/// Deeper than this, a schema's part is a plain object.
pub const MAX_DEPTH: usize = 10;
/// A schema bigger than this (as JSON) isn't offered: it would take most of
/// a small model's tool budget by itself.
pub const MAX_BYTES: usize = 16 * 1024;

/// The keywords every provider reads.
const KEEP: &[&str] = &[
    "type",
    "description",
    "title",
    "properties",
    "required",
    "items",
    "enum",
    "anyOf",
    "nullable",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "pattern",
    "format",
    "default",
];

/// The string formats Gemini takes (others dropped, the type kept).
const FORMATS: &[&str] = &["date-time", "date", "time", "email", "uri", "uuid", "enum"];

/// The types JSON Schema has.
const TYPES: &[&str] = &["object", "array", "string", "number", "integer", "boolean"];

/// `schema`, made safe ([module docs](self)), or why it can't be. Pure.
pub fn make_safe(schema: &Value) -> Result<Value, String> {
    let Value::Object(top) = schema else {
        return Err("Its inputs aren't described as an object.".into());
    };
    match top.get("type") {
        None => {}
        Some(Value::String(t)) if t == "object" => {}
        Some(Value::Array(ts)) if ts.iter().any(|t| t == "object") => {}
        Some(_) => return Err("Its inputs aren't described as an object.".into()),
    }
    let mut out = clean(schema, schema, 0)?;
    let obj = out.as_object_mut().expect("an object stays an object");
    obj.insert("type".into(), json!("object"));
    obj.remove("nullable");
    if !obj.get("properties").is_some_and(Value::is_object) {
        obj.insert("properties".into(), json!({}));
    }
    let size = serde_json::to_string(&out).map_or(usize::MAX, |s| s.len());
    if size > MAX_BYTES {
        return Err(format!("Its inputs are described at {} KB, more than Yap sends a model ({} KB).", size.div_ceil(1024), MAX_BYTES / 1024));
    }
    Ok(out)
}

/// A `$ref` inside `root`, if it's local.
fn resolve<'a>(root: &'a Value, reference: &str) -> Result<&'a Value, String> {
    let Some(path) = reference.strip_prefix("#/") else {
        return if reference == "#" {
            Ok(root)
        } else {
            Err("Its inputs refer to a schema somewhere else, which Yap doesn't fetch.".into())
        };
    };
    let mut at = root;
    for part in path.split('/') {
        let key = part.replace("~1", "/").replace("~0", "~");
        at = at.get(&key).ok_or_else(|| "Its inputs refer to a part of their schema that isn't there.".to_string())?;
    }
    Ok(at)
}

/// One part of a schema, cleaned (`depth`: how deep it is).
fn clean(node: &Value, root: &Value, depth: usize) -> Result<Value, String> {
    let Value::Object(map) = node else {
        // `true` / `false` schemas, or anything else: any value.
        return Ok(json!({}));
    };
    if depth > MAX_DEPTH {
        return Ok(json!({ "type": "object" }));
    }
    if let Some(Value::String(r)) = map.get("$ref") {
        let target = resolve(root, r)?;
        let mut resolved = clean(target, root, depth + 1)?;
        // The ref's own description wins over its target's.
        if let (Some(d), Some(obj)) = (map.get("description"), resolved.as_object_mut()) {
            obj.insert("description".into(), d.clone());
        }
        return Ok(resolved);
    }
    let mut out = Map::new();
    // `allOf`: objects merged (their properties and required), else the first.
    if let Some(Value::Array(all)) = map.get("allOf") {
        let mut merged = Map::new();
        let mut props = Map::new();
        let mut required: Vec<Value> = Vec::new();
        for part in all {
            let c = clean(part, root, depth + 1)?;
            if let Value::Object(c) = c {
                if let Some(Value::Object(p)) = c.get("properties") {
                    props.extend(p.clone());
                }
                if let Some(Value::Array(r)) = c.get("required") {
                    required.extend(r.iter().cloned());
                }
                for (k, v) in c {
                    if k != "properties" && k != "required" {
                        merged.entry(k).or_insert(v);
                    }
                }
            }
        }
        if !props.is_empty() {
            merged.insert("properties".into(), Value::Object(props));
        }
        if !required.is_empty() {
            merged.insert("required".into(), Value::Array(required));
        }
        out.extend(merged);
    }
    for (key, value) in map {
        match key.as_str() {
            "type" => {
                let (t, nullable) = one_type(value);
                if let Some(t) = t {
                    out.insert("type".into(), json!(t));
                }
                if nullable {
                    out.insert("nullable".into(), json!(true));
                }
            }
            "properties" => {
                let Value::Object(props) = value else { continue };
                let mut cleaned = Map::new();
                for (name, prop) in props {
                    cleaned.insert(name.clone(), clean(prop, root, depth + 1)?);
                }
                out.insert("properties".into(), Value::Object(cleaned));
            }
            "items" => {
                // A tuple's first type stands for them all.
                let item = match value {
                    Value::Array(list) => list.first().cloned().unwrap_or(json!({})),
                    other => other.clone(),
                };
                out.insert("items".into(), clean(&item, root, depth + 1)?);
            }
            "anyOf" | "oneOf" => {
                let Value::Array(list) = value else { continue };
                let mut options = Vec::new();
                let mut nullable = false;
                for option in list {
                    if option.get("type").and_then(Value::as_str) == Some("null") {
                        nullable = true;
                        continue;
                    }
                    options.push(clean(option, root, depth + 1)?);
                }
                match options.len() {
                    0 => {}
                    // One option (and maybe null): just it.
                    1 => {
                        if let Value::Object(only) = options.remove(0) {
                            for (k, v) in only {
                                out.entry(k).or_insert(v);
                            }
                        }
                    }
                    _ => {
                        out.insert("anyOf".into(), Value::Array(options));
                    }
                }
                if nullable {
                    out.insert("nullable".into(), json!(true));
                }
            }
            "const" => {
                out.insert("enum".into(), json!([value]));
            }
            "enum" => {
                if let Value::Array(values) = value {
                    out.insert("enum".into(), Value::Array(values.clone()));
                }
            }
            "format" => {
                if value.as_str().is_some_and(|f| FORMATS.contains(&f)) {
                    out.insert("format".into(), value.clone());
                }
            }
            "description" | "title" => {
                if let Value::String(s) = value {
                    out.insert(key.clone(), json!(s.chars().take(1_000).collect::<String>()));
                }
            }
            k if KEEP.contains(&k) => {
                out.entry(k.to_string()).or_insert_with(|| value.clone());
            }
            // Everything else ($schema, $id, $defs, definitions, examples,
            // additionalProperties, patternProperties, if/then/else, not…).
            _ => {}
        }
    }
    // A `const`/`enum` of non-strings: Gemini takes string enums only.
    if let Some(Value::Array(values)) = out.get("enum") {
        if values.is_empty() || !values.iter().all(Value::is_string) {
            out.remove("enum");
        } else if !out.contains_key("type") {
            out.insert("type".into(), json!("string"));
        }
    }
    // Infer a type the providers need when the shape says it.
    if !out.contains_key("type") && !out.contains_key("anyOf") {
        if out.contains_key("properties") {
            out.insert("type".into(), json!("object"));
        } else if out.contains_key("items") {
            out.insert("type".into(), json!("array"));
        }
    }
    match out.get("type").and_then(Value::as_str) {
        Some("array") if !out.contains_key("items") => {
            out.insert("items".into(), json!({ "type": "string" }));
        }
        Some("object") => {
            let names: Vec<String> = out.get("properties").and_then(Value::as_object).map(|p| p.keys().cloned().collect()).unwrap_or_default();
            if let Some(Value::Array(req)) = out.get_mut("required") {
                req.retain(|r| r.as_str().is_some_and(|r| names.iter().any(|n| n == r)));
                let mut seen = std::collections::BTreeSet::new();
                req.retain(|r| seen.insert(r.as_str().unwrap_or_default().to_string()));
                if req.is_empty() {
                    out.remove("required");
                }
            } else {
                out.remove("required");
            }
        }
        // A string's numeric bounds, an array's string ones: what doesn't
        // apply is dropped (Gemini refuses some).
        Some("string") => {
            for k in ["minimum", "maximum", "minItems", "maxItems"] {
                out.remove(k);
            }
        }
        Some("number") | Some("integer") => {
            for k in ["minLength", "maxLength", "pattern", "minItems", "maxItems"] {
                out.remove(k);
            }
        }
        _ => {}
    }
    if out.get("type").and_then(Value::as_str) != Some("object") {
        out.remove("required");
    }
    Ok(Value::Object(out))
}

/// `type`'s one type, and whether it also allows null.
fn one_type(value: &Value) -> (Option<&str>, bool) {
    match value {
        Value::String(t) if TYPES.contains(&t.as_str()) => (Some(t.as_str()), false),
        Value::String(t) if t == "null" => (None, true),
        Value::Array(ts) => {
            let nullable = ts.iter().any(|t| t == "null");
            let first = ts.iter().filter_map(Value::as_str).find(|t| TYPES.contains(t));
            (first, nullable)
        }
        _ => (None, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_schema_comes_through_and_an_empty_one_becomes_an_object() {
        let s = json!({
            "type": "object",
            "properties": { "area": { "type": "string", "description": "A room" }, "level": { "type": "integer", "minimum": 0, "maximum": 100 } },
            "required": ["area"]
        });
        assert_eq!(make_safe(&s).unwrap(), s);
        assert_eq!(make_safe(&json!({})).unwrap(), json!({ "type": "object", "properties": {} }));
        assert_eq!(make_safe(&json!({ "properties": {} })).unwrap(), json!({ "type": "object", "properties": {} }));
    }

    #[test]
    fn what_providers_refuse_is_dropped() {
        let s = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "when": { "type": ["string", "null"], "format": "date-time", "examples": ["2026-11-08T10:00:00Z"] },
                "mode": { "const": "fast" },
                "count": { "type": "integer", "enum": [1, 2, 3], "minLength": 3 },
                "tags": { "type": "array" },
                "pick": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                "maybe": { "anyOf": [{ "type": "string", "pattern": "^a" }, { "type": "null" }] },
                "rgb": { "type": "string", "format": "color-hex" }
            },
            "required": ["when", "ghost", "when"],
            "patternProperties": { "^x-": {} },
            "if": { "properties": { "mode": { "const": "fast" } } }
        });
        let out = make_safe(&s).unwrap();
        assert_eq!(
            out,
            json!({
                "type": "object",
                "properties": {
                    "when": { "type": "string", "nullable": true, "format": "date-time" },
                    "mode": { "enum": ["fast"], "type": "string" },
                    "count": { "type": "integer" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "pick": { "anyOf": [{ "type": "string" }, { "type": "number" }] },
                    "maybe": { "type": "string", "pattern": "^a", "nullable": true },
                    "rgb": { "type": "string" }
                },
                "required": ["when"]
            })
        );
    }

    #[test]
    fn local_refs_are_resolved_and_recursion_is_cut() {
        let s = json!({
            "type": "object",
            "$defs": {
                "light": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] },
                "tree": { "type": "object", "properties": { "child": { "$ref": "#/$defs/tree" } } }
            },
            "properties": {
                "target": { "$ref": "#/$defs/light", "description": "Which light" },
                "old": { "$ref": "#/definitions/legacy" },
                "tree": { "$ref": "#/$defs/tree" }
            },
            "definitions": { "legacy": { "type": "boolean" } }
        });
        let out = make_safe(&s).unwrap();
        assert_eq!(out["properties"]["target"], json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"], "description": "Which light" }));
        assert_eq!(out["properties"]["old"], json!({ "type": "boolean" }));
        assert!(out.get("$defs").is_none() && out.get("definitions").is_none());
        // Recursion ends as a plain object, MAX_DEPTH down.
        let mut at = &out["properties"]["tree"];
        let mut depth = 0;
        while let Some(child) = at.get("properties").and_then(|p| p.get("child")) {
            at = child;
            depth += 1;
        }
        assert!(depth <= MAX_DEPTH, "{depth}");
        assert_eq!(at, &json!({ "type": "object" }));
        // A remote one is refused, with a reason.
        let err = make_safe(&json!({ "type": "object", "properties": { "x": { "$ref": "https://example.com/s.json" } } })).unwrap_err();
        assert!(err.contains("somewhere else"), "{err}");
    }

    #[test]
    fn an_all_of_merges_and_what_isnt_an_object_or_is_too_big_is_refused() {
        let s = json!({
            "allOf": [
                { "type": "object", "properties": { "a": { "type": "string" } }, "required": ["a"] },
                { "properties": { "b": { "type": "number" } } }
            ]
        });
        assert_eq!(
            make_safe(&s).unwrap(),
            json!({ "type": "object", "properties": { "a": { "type": "string" }, "b": { "type": "number" } }, "required": ["a"] })
        );
        assert!(make_safe(&json!({ "type": "string" })).is_err());
        assert!(make_safe(&json!([1, 2])).is_err());
        let mut props = Map::new();
        for i in 0..400 {
            props.insert(format!("field_{i}"), json!({ "type": "string", "description": "A field with a long description of what goes in it." }));
        }
        let err = make_safe(&json!({ "type": "object", "properties": props })).unwrap_err();
        assert!(err.contains("KB"), "{err}");
    }

    #[test]
    fn a_deep_schema_is_cut_and_long_descriptions_are_capped() {
        let mut s = json!({ "type": "string" });
        for _ in 0..20 {
            s = json!({ "type": "object", "properties": { "x": s } });
        }
        let out = make_safe(&s).unwrap();
        let text = serde_json::to_string(&out).unwrap();
        assert!(text.matches("\"x\"").count() <= MAX_DEPTH + 1);
        let long = json!({ "type": "object", "properties": { "q": { "type": "string", "description": "y".repeat(5_000) } } });
        assert_eq!(make_safe(&long).unwrap()["properties"]["q"]["description"].as_str().unwrap().len(), 1_000);
    }
}
