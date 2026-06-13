use clise_core::config::CliseConfig;
use clise_core::format::{detect, parse};
use clise_core::schema::SchemaFetcher;
use serde_json::Value;
use std::fs;

pub async fn run(
    file: String,
    schema_opt: Option<String>,
    quiet: bool,
    catalog_match: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read and parse data file
    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) => {
            if !quiet {
                eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                eprintln!(
                    "      \x1b[1;31mError:\x1b[0m Failed to read file '{}': {}\n",
                    file, e
                );
            }
            std::process::exit(1);
        }
    };

    let format = detect(&file, &content);
    let data_val = match parse(&content, format) {
        Ok(val) => val,
        Err(e) => {
            if !quiet {
                eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                eprintln!(
                    "      \x1b[1;31mError:\x1b[0m Failed to parse data file as {:?}: {}\n",
                    format, e
                );
            }
            std::process::exit(1);
        }
    };

    // 2. Resolve JSON Schema
    let schema_val = if let Some(schema_path) = schema_opt {
        if schema_path.starts_with("http://") || schema_path.starts_with("https://") {
            let fetcher = SchemaFetcher::new()?;
            match fetcher.fetch_schema(&schema_path).await {
                Ok(s) => s,
                Err(e) => {
                    if !quiet {
                        eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                        eprintln!(
                            "      \x1b[1;31mError:\x1b[0m Failed to fetch schema from URL '{}': {}\n",
                            schema_path, e
                        );
                    }
                    std::process::exit(1);
                }
            }
        } else {
            // Local file
            let s_content = match fs::read_to_string(&schema_path) {
                Ok(c) => c,
                Err(e) => {
                    if !quiet {
                        eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                        eprintln!(
                            "      \x1b[1;31mError:\x1b[0m Failed to read schema file '{}': {}\n",
                            schema_path, e
                        );
                    }
                    std::process::exit(1);
                }
            };
            let s_format = detect(&schema_path, &s_content);
            match parse(&s_content, s_format) {
                Ok(s) => s,
                Err(e) => {
                    if !quiet {
                        eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                        eprintln!(
                            "      \x1b[1;31mError:\x1b[0m Failed to parse schema file as {:?}: {}\n",
                            s_format, e
                        );
                    }
                    std::process::exit(1);
                }
            }
        }
    } else {
        // Fallback search logic
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
                if !quiet {
                    eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                    eprintln!(
                        "      \x1b[1;31mError:\x1b[0m No schema mapping found for file '{}'. Please specify one using --schema.\n",
                        file
                    );
                }
                std::process::exit(1);
            }
        };

        if !quiet {
            println!("Using schema: {}", schema_url);
        }

        let fetcher = SchemaFetcher::new()?;
        match fetcher.fetch_schema(&schema_url).await {
            Ok(s) => s,
            Err(e) => {
                if !quiet {
                    eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                    eprintln!(
                        "      \x1b[1;31mError:\x1b[0m Failed to fetch schema from URL '{}': {}\n",
                        schema_url, e
                    );
                }
                std::process::exit(1);
            }
        }
    };

    // 3. Perform JSON Schema validation
    let schema_val_clone = schema_val.clone();
    let data_val_clone = data_val.clone();

    let validation_res = tokio::task::spawn_blocking(move || {
        jsonschema::validator_for(&schema_val_clone).map(|validator| {
            let mut errors = Vec::new();
            for error in validator.iter_errors(&data_val_clone) {
                errors.push((
                    error.instance_path().to_string(),
                    format_validation_error(&error),
                ));
            }
            errors
        })
    })
    .await?;

    match validation_res {
        Ok(errors) => {
            if !errors.is_empty() {
                if !quiet {
                    eprintln!("\n\x1b[1;31mValidation failed\x1b[0m\n");
                    for (i, (path, err_msg)) in errors.iter().enumerate() {
                        eprintln!(
                            "  \x1b[1;33m[{}]\x1b[0m \x1b[1mPath :\x1b[0m {}",
                            i + 1,
                            path
                        );
                        eprintln!("      \x1b[1;31mError:\x1b[0m {}\n", err_msg);
                    }
                }
                std::process::exit(1);
            }
        }
        Err(e) => {
            if !quiet {
                eprintln!("\x1b[1;31mError compiling JSON Schema:\x1b[0m {}", e);
            }
            std::process::exit(1);
        }
    }

    if !quiet {
        println!("\x1b[1;32mValidation successful\x1b[0m");
    }

    Ok(())
}

fn format_validation_error(error: &jsonschema::ValidationError) -> String {
    let instance_str = match &**error.instance() {
        Value::Object(_) => "[Object]".to_string(),
        Value::Array(_) => "[Array]".to_string(),
        Value::String(s) => {
            if s.len() > 60 {
                format!("\"{}...\"", &s[..57])
            } else {
                format!("\"{}\"", s)
            }
        }
        other => other.to_string(),
    };

    let msg = error.to_string();
    let raw_instance = error.instance().to_string();
    if msg.starts_with(&raw_instance) {
        format!("{}{}", instance_str, &msg[raw_instance.len()..])
    } else {
        msg
    }
}
