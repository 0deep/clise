use crate::commands::SchemaCommands;
use clise_core::config::CliseConfig;
use clise_core::schema::SchemaFetcher;
use std::fs;

pub async fn run(cmd: SchemaCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SchemaCommands::List => {
            let config = CliseConfig::load_or_init();
            if config.schema_mappings.is_empty() {
                println!("No custom schema mappings configured.");
            } else {
                println!("Active custom schema mappings:");
                for mapping in &config.schema_mappings {
                    println!(
                        "  Pattern: '{}' -> URL: {} ({})",
                        mapping.file, mapping.url, mapping.name
                    );
                }
            }
        }
        SchemaCommands::Show { file } => {
            let config = CliseConfig::load_or_init();
            let mapping = config.get_mapping(&file);
            let url = if let Some(m) = mapping {
                Some((m.url, format!("custom: {}", m.name)))
            } else {
                let fetcher = SchemaFetcher::new()?;
                if let Ok(catalog) = fetcher.fetch_catalog().await {
                    SchemaFetcher::find_schema_url(&catalog, &file)
                        .map(|entry| (entry.url, entry.name))
                } else {
                    None
                }
            };

            match url {
                Some((u, source)) => {
                    println!("Mapped Schema URL: {}", u);
                    println!("Source: {}", source);
                }
                None => {
                    println!("No schema mapped for file '{}'", file);
                }
            }
        }
        SchemaCommands::Map {
            pattern,
            schema_url,
            name,
        } => {
            let mut config = CliseConfig::load_or_init();
            let display_name = name.unwrap_or_else(|| {
                schema_url
                    .split('/')
                    .last()
                    .unwrap_or(&schema_url)
                    .to_string()
            });

            config.update_mapping(
                pattern.clone(),
                schema_url.clone(),
                display_name.clone(),
                false,
            );
            config.save()?;
            println!(
                "Successfully mapped pattern '{}' to schema '{}' (Name: {})",
                pattern, schema_url, display_name
            );
        }
        SchemaCommands::Clean => {
            let cache_dir = directories::ProjectDirs::from("", "", "clise")
                .map(|dirs| dirs.cache_dir().to_path_buf())
                .ok_or("Could not determine cache directory")?;

            let catalog_path = cache_dir.join("catalog.json");
            if catalog_path.exists() {
                fs::remove_file(&catalog_path)?;
            }

            let schemas_dir = cache_dir.join("schemas");
            if schemas_dir.exists() {
                fs::remove_dir_all(&schemas_dir)?;
                fs::create_dir_all(&schemas_dir)?;
            }

            println!("Successfully cleaned schema cache.");
        }
    }
    Ok(())
}
