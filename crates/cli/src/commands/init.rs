use clise_core::config::CliseConfig;
use clise_core::format::{Format, detect, serialize};
use clise_core::schema::SchemaFetcher;
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

pub async fn run(
    file: String,
    schema_opt: Option<String>,
    force: bool,
    format_opt: Option<String>,
    catalog_match: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Check duplicate file
    let path = Path::new(&file);
    if path.exists() && !force {
        eprintln!(
            "Error: File '{}' already exists. Use --force to overwrite.",
            file
        );
        std::process::exit(1);
    }

    // 2. Resolve JSON Schema (similar to validate.rs)
    let schema_val = if let Some(schema_path) = schema_opt {
        if schema_path.starts_with("http://") || schema_path.starts_with("https://") {
            let fetcher = SchemaFetcher::new()?;
            match fetcher.fetch_schema(&schema_path).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error fetching schema from URL '{}': {}", schema_path, e);
                    std::process::exit(1);
                }
            }
        } else {
            // Local file
            let s_content = match fs::read_to_string(&schema_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading schema file '{}': {}", schema_path, e);
                    std::process::exit(1);
                }
            };
            let s_format = detect(&schema_path, &s_content);
            match clise_core::format::parse(&s_content, s_format) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "Error parsing schema file '{}' as {:?}: {}",
                        schema_path, s_format, e
                    );
                    std::process::exit(1);
                }
            }
        }
    } else {
        // Fallback search logic using CliseConfig and Catalog
        let config = CliseConfig::load_or_init();
        let match_target = catalog_match.as_deref().unwrap_or(&file);
        let mapping = config.get_mapping(match_target);
        let url = if let Some(m) = mapping {
            Some(m.url)
        } else {
            let fetcher = SchemaFetcher::new()?;
            if let Ok(catalog) = fetcher.fetch_catalog().await {
                SchemaFetcher::find_schema_url(&catalog, match_target).map(|entry| entry.url)
            } else {
                None
            }
        };

        let schema_url = match url {
            Some(u) => u,
            None => {
                eprintln!(
                    "Error: No schema mapping found for file '{}'. Please specify one using --schema.",
                    file
                );
                std::process::exit(1);
            }
        };

        println!("Using schema: {}", schema_url);

        let fetcher = SchemaFetcher::new()?;
        match fetcher.fetch_schema(&schema_url).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error fetching schema from URL '{}': {}", schema_url, e);
                std::process::exit(1);
            }
        }
    };

    // 3. Extract template data based on schema's required fields
    let template_data = generate_skeleton(&schema_val);

    // 4. Formatting and serialization
    let format = if let Some(fmt_str) = format_opt {
        match fmt_str.to_lowercase().as_str() {
            "json" => Format::Json,
            "jsonc" => Format::Jsonc,
            "yaml" | "yml" => Format::Yaml,
            "toml" => Format::Toml,
            other => {
                eprintln!(
                    "Error: Unsupported format '{}'. Supported: json, jsonc, yaml, toml",
                    other
                );
                std::process::exit(1);
            }
        }
    } else {
        // Fallback to detect by filename
        let ext = Path::new(&file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "json" => Format::Json,
            "jsonc" => Format::Jsonc,
            "yaml" | "yml" => Format::Yaml,
            "toml" => Format::Toml,
            _ => Format::Json,
        }
    };

    let serialized = match serialize(&template_data, format, None, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error serializing template: {}", e);
            std::process::exit(1);
        }
    };

    // 5. Write to file
    fs::write(&file, serialized)?;
    println!("Initialized '{}' successfully using schema template.", file);

    Ok(())
}

fn generate_skeleton(schema_val: &Value) -> Value {
    let mut template_obj = Map::new();
    if let Some(properties) = schema_val.get("properties").and_then(|p| p.as_object()) {
        let mut required_fields: Vec<String> = schema_val
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        // Fallback: if required list is empty, treat all properties as required for template skeleton
        if required_fields.is_empty() {
            required_fields = properties.keys().cloned().collect();
        }

        for field in &required_fields {
            if let Some(prop_info) = properties.get(field) {
                let default_val = prop_info.get("default").cloned().unwrap_or_else(|| {
                    let type_str = prop_info
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("string");
                    match type_str {
                        "string" => Value::String("".to_string()),
                        "integer" | "number" => Value::Number(0.into()),
                        "boolean" => Value::Bool(false),
                        "array" => Value::Array(Vec::new()),
                        "object" => Value::Object(Map::new()),
                        _ => Value::Null,
                    }
                });
                template_obj.insert(field.clone(), default_val);
            }
        }
    }
    Value::Object(template_obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_generate_skeleton_basic() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "default": "test-project"
                },
                "version": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                },
                "ok": {
                    "type": "boolean",
                    "default": true
                },
                "ignored_field": {
                    "type": "string"
                }
            },
            "required": ["name", "version", "count", "ok"]
        });

        let skeleton = generate_skeleton(&schema);
        assert_eq!(
            skeleton.get("name").unwrap().as_str().unwrap(),
            "test-project"
        );
        assert_eq!(skeleton.get("version").unwrap().as_str().unwrap(), "");
        assert_eq!(skeleton.get("count").unwrap().as_i64().unwrap(), 0);
        assert_eq!(skeleton.get("ok").unwrap().as_bool().unwrap(), true);
        assert!(skeleton.get("ignored_field").is_none());
    }

    #[test]
    fn test_generate_skeleton_no_required_fallback() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "default": "default-name"
                },
                "version": {
                    "type": "string"
                }
            }
        });

        let skeleton = generate_skeleton(&schema);
        assert_eq!(
            skeleton.get("name").unwrap().as_str().unwrap(),
            "default-name"
        );
        assert_eq!(skeleton.get("version").unwrap().as_str().unwrap(), "");
    }
}
