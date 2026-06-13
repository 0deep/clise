use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SchemaMapping {
    pub file: String,
    pub url: String,
    pub name: String,
    pub downloaded: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CliseConfig {
    /// List of user-defined schema mappings
    pub schema_mappings: Vec<SchemaMapping>,
}

impl CliseConfig {
    pub fn load_or_init() -> Self {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path).unwrap_or_default();
            if let Ok(mappings) = serde_json::from_str::<Vec<SchemaMapping>>(&content) {
                return Self {
                    schema_mappings: mappings,
                };
            }
            if let Ok(config) = serde_json::from_str::<CliseConfig>(&content) {
                return config;
            }
        }

        let config = Self::default();
        let _ = config.save();
        config
    }

    pub fn save(&self) -> std::io::Result<()> {
        let config_path = Self::config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            serde_json::to_string_pretty(&self.schema_mappings).map_err(std::io::Error::other)?;
        std::fs::write(config_path, content)
    }

    pub fn config_path() -> PathBuf {
        directories::ProjectDirs::from("", "", "clise")
            .map(|dirs| dirs.config_dir().join("schemas.json"))
            .unwrap_or_else(|| PathBuf::from("schemas.json"))
    }

    pub fn update_mapping(&mut self, file: String, url: String, name: String, downloaded: bool) {
        if let Some(existing) = self.schema_mappings.iter_mut().find(|m| m.file == file) {
            existing.url = url;
            existing.name = name;
            existing.downloaded = downloaded;
        } else {
            self.schema_mappings.push(SchemaMapping {
                file,
                url,
                name,
                downloaded,
            });
        }
    }

    pub fn get_mapping(&self, file: &str) -> Option<SchemaMapping> {
        // First pass: exact match
        if let Some(m) = self.schema_mappings.iter().find(|m| m.file == file) {
            return Some(m.clone());
        }

        // Second pass: glob match
        self.schema_mappings
            .iter()
            .find(|m| crate::schema::match_glob(&m.file, file))
            .cloned()
    }
}
