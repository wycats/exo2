use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value as TomlValue};

pub fn read_json(path: &Path, pointer: Option<&str>) -> Result<String> {
    let content = std::fs::read_to_string(path).context("Failed to read JSON file")?;
    let json: JsonValue = serde_json::from_str(&content).context("Failed to parse JSON")?;

    if let Some(ptr) = pointer {
        let val = json.pointer(ptr).context("Pointer not found")?;
        if let Some(s) = val.as_str() {
            Ok(s.to_string())
        } else {
            Ok(serde_json::to_string_pretty(val)?)
        }
    } else {
        Ok(serde_json::to_string_pretty(&json)?)
    }
}

pub fn write_json(path: &Path, pointer: &str, value: &str) -> Result<()> {
    crate::utils::edit_file_with_permissions(path, |content| {
        let content = if content.trim().is_empty() {
            "{}".to_string()
        } else {
            content.to_string()
        };

        let mut json: JsonValue = serde_json::from_str(&content)
            .unwrap_or_else(|_| JsonValue::Object(serde_json::Map::new()));

        // Try to parse the input value as JSON, otherwise treat as string
        let parsed_value: JsonValue =
            serde_json::from_str(value).unwrap_or_else(|_| JsonValue::String(value.to_string()));

        if pointer.is_empty() || pointer == "/" {
            json = parsed_value;
        } else if let Some(target) = json.pointer_mut(pointer) {
            *target = parsed_value;
        } else {
            // Simple auto-vivification for top-level keys if pointer is like "/key"
            if pointer.starts_with('/') && !pointer[1..].contains('/') {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert(pointer[1..].to_string(), parsed_value);
                } else {
                    anyhow::bail!("Root is not an object, cannot insert key");
                }
            } else {
                anyhow::bail!(
                    "JSON pointer target does not exist (deep auto-creation not implemented yet)"
                );
            }
        }

        Ok(serde_json::to_string_pretty(&json)?)
    })?;
    Ok(())
}

pub fn read_toml(path: &Path, key: Option<&str>) -> Result<String> {
    let content = std::fs::read_to_string(path).context("Failed to read TOML file")?;
    let doc = content
        .parse::<DocumentMut>()
        .context("Failed to parse TOML")?;

    if let Some(k) = key {
        let mut current = doc.as_item();
        for part in k.split('.') {
            current = current.get(part).context("Key not found")?;
        }
        // If it's a value, print raw, else print toml representation
        current.as_value().map_or_else(
            || Ok(current.to_string()),
            |v| {
                v.as_str()
                    .map_or_else(|| Ok(v.to_string()), |s| Ok(s.to_string()))
            },
        )
    } else {
        Ok(doc.to_string())
    }
}

pub fn read_toml_as_json(path: &Path, key: Option<&str>) -> Result<JsonValue> {
    let content = std::fs::read_to_string(path).context("Failed to read TOML file")?;

    // For machine-readable output, use `toml` (not toml_edit) and serialize to JSON.
    let doc: toml::Value = toml::from_str(&content).context("Failed to parse TOML")?;

    let value = if let Some(k) = key {
        let mut current = &doc;
        for part in k.split('.') {
            current = current
                .get(part)
                .with_context(|| format!("Key not found: {k}"))?;
        }
        current
    } else {
        &doc
    };

    serde_json::to_value(value).context("Failed to serialize TOML value as JSON")
}

pub fn write_toml(path: &Path, key: &str, value: &str) -> Result<()> {
    crate::utils::edit_file_with_permissions(path, |content| {
        let mut doc = content.parse::<DocumentMut>().unwrap_or_default();

        // Try to parse as TOML value (e.g. numbers, booleans, inline arrays)
        // toml_edit::Value::from_str is not available directly like that, we need to parse a mini doc
        let toml_val = value
            .parse::<TomlValue>()
            .unwrap_or_else(|_| TomlValue::from(value));

        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() {
            anyhow::bail!("Key cannot be empty");
        }

        let mut current = doc.as_table_mut();
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                current[part] = Item::Value(toml_val.clone());
            } else {
                let entry = &mut current[part];
                if entry.is_none() {
                    *entry = Item::Table(toml_edit::Table::new());
                }
                current = entry
                    .as_table_mut()
                    .context("Path segment is not a table")?;
            }
        }

        Ok(doc.to_string())
    })?;
    Ok(())
}
